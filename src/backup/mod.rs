//! 并发处理备份队列，并保留尚未完成的相册整理记录。
mod album;
mod auth;
mod client;
mod error;
mod protocol;

pub use album::{AlbumState, AlbumTarget, parse as parse_album};
pub use auth::{Credentials, STORE_URL, exchange_embedded};
pub use client::Phase;
pub use error::{Error, ErrorKind};

use crate::queue::MediaFile;
use client::{Outcome, PhotosClient, hash_file};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

#[derive(Debug, Clone)]
pub enum ItemOutcome {
    Pending,
    Uploaded,
    Exists,
    Failed(Error),
    Uncertain(Error),
}

impl ItemOutcome {
    pub fn needs_processing(&self) -> bool {
        matches!(self, Self::Pending | Self::Failed(_) | Self::Uncertain(_))
    }
}

pub enum Event {
    Paused(u64),
    Stage {
        path: PathBuf,
        phase: Phase,
    },
    Transfer {
        path: PathBuf,
        sent: u64,
        total: u64,
    },
    Item {
        path: PathBuf,
        outcome: ItemOutcome,
    },
    Finished(Result<Report, Error>),
}

#[derive(Default)]
pub struct PauseControl {
    requested: AtomicBool,
    generation: AtomicU64,
}

impl PauseControl {
    pub fn request(&self) -> u64 {
        let generation = self
            .generation
            .fetch_add(1, Ordering::SeqCst)
            .wrapping_add(1);
        self.requested.store(true, Ordering::SeqCst);
        generation
    }

    pub fn resume(&self) {
        self.requested.store(false, Ordering::SeqCst);
    }
}

struct TaskControl {
    cancel: Arc<AtomicBool>,
    paused: Arc<PauseControl>,
    events: async_channel::Sender<Event>,
}

pub struct Report {
    pub cancelled: bool,
    pub stopped: Option<Error>,
    pub album: AlbumState,
}

pub fn organize_album(credentials: Credentials, mut album: AlbumState) -> AlbumState {
    let keys: Vec<_> = album.pending_keys.iter().cloned().collect();
    let mut target = album.target.clone();
    let result = PhotosClient::new(credentials).and_then(|mut client| {
        client.archive(&album.target, &keys, |key| {
            target = AlbumTarget::Existing(key.to_owned());
        })
    });
    album.target = target;
    album.error = result.err();
    if album.error.is_none() {
        album.pending_keys.clear();
    }
    album
}

pub fn verify_account(credentials: Credentials) -> Result<Credentials, Error> {
    let mut client = PhotosClient::new(credentials.clone())?;
    client.validate()?;
    Ok(credentials)
}

pub fn run(
    files: Vec<MediaFile>,
    credentials: Credentials,
    cancel: Arc<AtomicBool>,
    paused: Arc<PauseControl>,
    events: &async_channel::Sender<Event>,
    threads: usize,
    album: AlbumState,
) -> Result<Report, Error> {
    let queue = Mutex::new(files.into_iter());
    // 相同内容的工作线程共享同一次云端检查和上传结果。
    let seen = Mutex::new(HashMap::<[u8; 20], Arc<OnceLock<Result<Outcome, Error>>>>::new());
    let album = Mutex::new(album);
    let stopped = Mutex::new(None);
    let control = Arc::new(TaskControl {
        cancel: cancel.clone(),
        paused,
        events: events.clone(),
    });

    std::thread::scope(|scope| -> Result<(), Error> {
        let mut workers = Vec::new();
        for _ in 0..threads {
            let credentials = credentials.clone();
            let control = control.clone();
            let queue = &queue;
            let seen = &seen;
            let album = &album;
            let stopped = &stopped;
            let cancel = &cancel;
            workers.push(scope.spawn(move || -> Result<(), Error> {
                let mut client = PhotosClient::new(credentials)?;
                loop {
                    if stopped.lock().map_err(|e| e.to_string())?.is_some()
                        || cancel.load(Ordering::Relaxed)
                    {
                        break;
                    }
                    let file = queue.lock().map_err(|e| e.to_string())?.next();
                    let Some(file) = file else {
                        break;
                    };
                    let report_stage = |phase| {
                        let _ = events.send_blocking(Event::Stage {
                            path: file.path.clone(),
                            phase,
                        });
                    };
                    report_stage(Phase::Hashing);
                    let result = hash_file(&file.path, &control).and_then(|sha| {
                        let cell = seen
                            .lock()
                            .map_err(|e| e.to_string())?
                            .entry(sha)
                            .or_insert_with(|| Arc::new(OnceLock::new()))
                            .clone();
                        let mut owner = false;
                        let result = cell
                            .get_or_init(|| {
                                owner = true;
                                let transfer_events = events.clone();
                                let path = file.path.clone();
                                let on_bytes: Arc<dyn Fn(u64, u64) + Send + Sync> =
                                    Arc::new(move |sent, total| {
                                        let _ = transfer_events.try_send(Event::Transfer {
                                            path: path.clone(),
                                            sent,
                                            total,
                                        });
                                    });
                                client.upload_one(
                                    &file,
                                    &sha,
                                    control.clone(),
                                    &report_stage,
                                    on_bytes,
                                )
                            })
                            .clone();
                        if owner {
                            result
                        } else {
                            report_stage(Phase::Checking);
                            result.map(|outcome| match outcome {
                                Outcome::Uploaded(key) | Outcome::Exists(key) => {
                                    Outcome::Exists(key)
                                }
                            })
                        }
                    });
                    let outcome = match result {
                        Ok(Outcome::Uploaded(key)) => {
                            album.lock().map_err(|e| e.to_string())?.record(key);
                            ItemOutcome::Uploaded
                        }
                        Ok(Outcome::Exists(key)) => {
                            album.lock().map_err(|e| e.to_string())?.record(key);
                            ItemOutcome::Exists
                        }
                        Err(error) if error.kind == ErrorKind::Cancelled => break,
                        Err(error) => {
                            if error.stops_run() {
                                stopped
                                    .lock()
                                    .map_err(|e| e.to_string())?
                                    .get_or_insert_with(|| error.clone());
                            }
                            if error.kind == ErrorKind::UncertainCommit {
                                ItemOutcome::Uncertain(error)
                            } else {
                                ItemOutcome::Failed(error)
                            }
                        }
                    };
                    let _ = events.send_blocking(Event::Item {
                        path: file.path,
                        outcome,
                    });
                }
                Ok(())
            }));
        }
        for worker in workers {
            if let Err(error) = worker
                .join()
                .map_err(|_| Error::from("上传工作线程异常终止"))
                .and_then(|result| result)
            {
                stopped
                    .lock()
                    .map_err(|e| e.to_string())?
                    .get_or_insert(error);
            }
        }
        Ok(())
    })?;

    let stopped = stopped.into_inner().map_err(|e| e.to_string())?;
    let mut album = album.into_inner().map_err(|e| e.to_string())?;
    if !cancel.load(Ordering::Relaxed) && stopped.is_none() && album.has_pending() {
        album = organize_album(credentials, album);
    }
    Ok(Report {
        cancelled: cancel.load(Ordering::Relaxed),
        stopped,
        album,
    })
}
