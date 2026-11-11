//! Bounded concurrent and cancelable upload pipeline. Retains every local original.
mod album;
mod auth;
mod client;
mod protocol;

pub use album::{AlbumTarget, parse as parse_album};
pub use auth::{Credentials, STORE_URL, exchange_embedded};
pub use client::Phase;

use crate::queue::MediaFile;
use client::{Outcome, PhotosClient, hash_file, wait_until_resumed};
use std::{
    collections::{HashMap, VecDeque},
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
    Failed(String),
    Uncertain(String),
}

#[derive(Debug)]
pub enum Event {
    Paused(u64),
    Stage {
        path: PathBuf,
        name: String,
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
    AlbumCreated(String),
    Finished(Result<Summary, String>),
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

#[derive(Debug, Default)]
pub struct Summary {
    pub uploaded: usize,
    pub skipped: usize,
    pub failed: usize,
    pub cancelled: bool,
    pub stopped: Option<String>,
    pub album_error: Option<String>,
    pub errors: Vec<String>,
}

/// Validate an exchanged master token against the Photos private API.
pub fn verify_account(credentials: Credentials) -> Result<Credentials, String> {
    let mut client = PhotosClient::new(credentials.clone())?;
    client.validate()?;
    Ok(credentials)
}

/// Each worker owns its HTTP client; only the queue, hash results and summary are shared.
pub fn run(
    files: Vec<MediaFile>,
    credentials: Credentials,
    cancel: Arc<AtomicBool>,
    paused: Arc<PauseControl>,
    events: &async_channel::Sender<Event>,
    threads: usize,
    album: AlbumTarget,
) -> Result<Summary, String> {
    let queue = Mutex::new(VecDeque::from(files));
    // OnceLock makes duplicate-content workers wait for the first result; failures
    // are never mistaken for successful cloud deduplication.
    let seen = Mutex::new(HashMap::<[u8; 20], Arc<OnceLock<Result<Outcome, String>>>>::new());
    let summary = Mutex::new(Summary::default());
    let media_keys = Mutex::new(Vec::<String>::new());
    let stop = AtomicBool::new(false);
    let control = Arc::new(TaskControl {
        cancel: cancel.clone(),
        paused,
        events: events.clone(),
    });

    std::thread::scope(|scope| -> Result<(), String> {
        let mut workers = Vec::new();
        for _ in 0..threads.clamp(1, 4) {
            let credentials = credentials.clone();
            let control = control.clone();
            let queue_ref = &queue;
            let seen_ref = &seen;
            let summary_ref = &summary;
            let media_keys_ref = &media_keys;
            let stop_ref = &stop;
            let cancel_ref = &cancel;
            workers.push(scope.spawn(move || -> Result<(), String> {
                let mut client = PhotosClient::new(credentials)?;
                loop {
                    if stop_ref.load(Ordering::Relaxed) || cancel_ref.load(Ordering::Relaxed) {
                        break;
                    }
                    let file = {
                        let mut remaining = queue_ref.lock().map_err(|e| e.to_string())?;
                        if stop_ref.load(Ordering::Relaxed) || cancel_ref.load(Ordering::Relaxed) {
                            None
                        } else {
                            remaining.pop_front()
                        }
                    };
                    let Some(file) = file else { break };
                    if wait_until_resumed(&control).is_err() {
                        break;
                    }
                    let report_stage = |phase| {
                        let _ = events.send_blocking(Event::Stage {
                            path: file.path.clone(),
                            name: file.name.clone(),
                            phase,
                        });
                    };
                    report_stage(Phase::Hashing);
                    let result = hash_file(&file.path, &control).and_then(|sha| {
                        let cell = seen_ref
                            .lock()
                            .map_err(|e| e.to_string())?
                            .entry(sha)
                            .or_insert_with(|| Arc::new(OnceLock::new()))
                            .clone();
                        let owner = std::cell::Cell::new(false);
                        let result = cell
                            .get_or_init(|| {
                                owner.set(true);
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
                        if !owner.get() {
                            report_stage(Phase::Checking);
                        }
                        result.map(|outcome| {
                            if owner.get() {
                                outcome
                            } else {
                                Outcome::Exists(outcome.media_key().to_owned())
                            }
                        })
                    });
                    // Keep cancelled, unconfirmed transfers pending for a later retry.
                    if cancel_ref.load(Ordering::Relaxed)
                        && matches!(&result, Err(error) if !error.contains("提交结果不确定"))
                    {
                        break;
                    }
                    let outcome = match &result {
                        Ok(Outcome::Uploaded(_)) => ItemOutcome::Uploaded,
                        Ok(Outcome::Exists(_)) => ItemOutcome::Exists,
                        Err(error) if error.contains("提交结果不确定") => {
                            ItemOutcome::Uncertain(error.clone())
                        }
                        Err(error) => ItemOutcome::Failed(error.clone()),
                    };
                    if let Ok(Outcome::Uploaded(key) | Outcome::Exists(key)) = &result {
                        media_keys_ref
                            .lock()
                            .map_err(|e| e.to_string())?
                            .push(key.clone());
                    }
                    let _ = events.send_blocking(Event::Item {
                        path: file.path.clone(),
                        outcome,
                    });
                    let mut report = summary_ref.lock().map_err(|e| e.to_string())?;
                    match result {
                        Ok(Outcome::Uploaded(_)) => report.uploaded += 1,
                        Ok(Outcome::Exists(_)) => report.skipped += 1,
                        Err(error) => {
                            report.failed += 1;
                            if report.errors.len() < 10 {
                                report.errors.push(format!("{}：{error}", file.name));
                            }
                            if error.contains("HTTP 401")
                                || error.contains("HTTP 403")
                                || error.contains("HTTP 429")
                                || error.contains("认证失败")
                                || error.contains("认证已失效")
                                || error.contains("设备绑定")
                            {
                                report.stopped.get_or_insert(error);
                                stop_ref.store(true, Ordering::Relaxed);
                            }
                        }
                    }
                }
                Ok(())
            }));
        }
        for worker in workers {
            worker
                .join()
                .map_err(|_| "上传工作线程异常终止".to_owned())??;
        }
        Ok(())
    })?;

    let mut report = summary.into_inner().map_err(|e| e.to_string())?;
    report.cancelled = cancel.load(Ordering::Relaxed);
    let keys = media_keys.into_inner().map_err(|e| e.to_string())?;
    if !report.cancelled
        && report.stopped.is_none()
        && !keys.is_empty()
        && !matches!(album, AlbumTarget::None)
    {
        let mut client = PhotosClient::new(credentials)?;
        let mut unique = std::collections::HashSet::new();
        let keys: Vec<_> = keys
            .into_iter()
            .filter(|key| unique.insert(key.clone()))
            .collect();
        if let Err(error) = client.archive(&album, &keys, |key| {
            let _ = events.send_blocking(Event::AlbumCreated(key.to_owned()));
        }) {
            report.album_error = Some(error);
        }
    }
    Ok(report)
}
