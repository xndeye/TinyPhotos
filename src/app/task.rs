use super::*;
use crate::backup::{AlbumTarget, ErrorKind, Event, Report};
use std::sync::atomic::Ordering;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum TaskState {
    #[default]
    Idle,
    Running,
    Pausing(u64),
    Paused,
    Stopping,
    OrganizingAlbum,
    Done,
}

impl TaskState {
    pub fn is_uploading(self) -> bool {
        matches!(
            self,
            Self::Running | Self::Pausing(_) | Self::Paused | Self::Stopping
        )
    }

    pub fn is_busy(self) -> bool {
        !matches!(self, Self::Idle | Self::Done)
    }
}

pub(super) struct TransferProgress {
    pub(super) phase: Phase,
    pub(super) sent: u64,
    pub(super) total: u64,
}

#[derive(Default)]
pub(super) struct BackupProgress {
    pub(super) transfers: HashMap<PathBuf, TransferProgress>,
    pub(super) total: usize,
    pub(super) processed: usize,
    pub(super) cancel: Arc<AtomicBool>,
    pub(super) paused: Arc<PauseControl>,
}

#[derive(Default)]
pub(super) struct BackupTask {
    pub(super) state: TaskState,
    pub(super) outcomes: HashMap<PathBuf, ItemOutcome>,
    pub(super) album: backup::AlbumState,
    pub(super) error: Option<backup::Error>,
    pub(super) cancelled: bool,
    pub(super) progress: BackupProgress,
    pub(super) filter: ResultFilter,
    pub(super) page: usize,
}

impl BackupTask {
    pub fn new(files: &[MediaFile]) -> Self {
        Self {
            outcomes: files
                .iter()
                .map(|file| (file.path.clone(), ItemOutcome::Pending))
                .collect(),
            ..Self::default()
        }
    }
}
impl UploadApp {
    pub(super) fn begin_backup(&mut self, _: &BeginBackup, _: &mut Window, cx: &mut Context<Self>) {
        self.continue_action(cx);
    }

    pub(super) fn backup_album_target(&self, cx: &App) -> Result<AlbumTarget, String> {
        let input = self.album_input.read(cx).value();
        if self.backup.state == TaskState::Done || self.backup.album.needs_verification() {
            self.backup.album.resolve(&input)
        } else {
            backup::parse_album(&input)
        }
    }

    fn record_authorization_problem(&mut self, error: &backup::Error) {
        if error.kind == ErrorKind::Authorization {
            self.account.problem = Some(error.clone());
        }
    }

    pub(super) fn toggle_pause(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.change_pause(cx);
    }

    pub(super) fn pause_hotkey(&mut self, _: &TogglePause, _: &mut Window, cx: &mut Context<Self>) {
        self.change_pause(cx);
    }

    fn change_pause(&mut self, cx: &mut Context<Self>) {
        match self.backup.state {
            TaskState::Running => {
                self.backup.state = TaskState::Pausing(self.backup.progress.paused.request());
                self.notice = "正在暂停，当前网络操作完成后生效".into();
            }
            TaskState::Pausing(_) | TaskState::Paused => {
                self.backup.progress.paused.resume();
                self.backup.state = TaskState::Running;
                self.notice = "已请求继续备份".into();
            }
            TaskState::Idle
            | TaskState::Stopping
            | TaskState::OrganizingAlbum
            | TaskState::Done => {
                return;
            }
        }
        cx.notify();
    }

    pub(super) fn continue_remaining(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let files = self
            .queue
            .files
            .iter()
            .filter(|file| {
                self.backup
                    .outcomes
                    .get(&file.path)
                    .is_some_and(ItemOutcome::needs_processing)
            })
            .cloned()
            .collect();
        self.continue_files(files, cx);
    }

    pub(super) fn retry_file(&mut self, file: MediaFile, cx: &mut Context<Self>) {
        self.continue_files(vec![file], cx);
    }

    pub(super) fn retry_album(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.busy() || self.account.problem.is_some() {
            return;
        }
        let target = match self.backup_album_target(cx) {
            Ok(target) => target,
            Err(error) => {
                self.notice = error;
                cx.notify();
                return;
            }
        };
        let Some(credentials) = self.account.credentials.clone() else {
            self.account.auth_open = true;
            cx.notify();
            return;
        };
        let mut album = std::mem::take(&mut self.backup.album);
        album.target = target;
        self.backup.state = TaskState::OrganizingAlbum;
        self.notice = "正在整理相册…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let album = cx
                .background_spawn(async move { backup::organize_album(credentials, album) })
                .await;
            let _ = this.update(cx, |app, cx| {
                app.backup.state = TaskState::Done;
                if let Some(error) = &album.error {
                    app.record_authorization_problem(error);
                    app.notice = "相册整理未完成，请检查错误后重试".into();
                } else {
                    app.notice = "相册整理完成".into();
                }
                app.backup.album = album;
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn return_to_review(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let AlbumTarget::Existing(key) = &self.backup.album.target {
            let value = key.clone();
            self.album_input
                .update(cx, |input, cx| input.set_value(value, window, cx));
        }
        self.backup.state = TaskState::Idle;
        self.notice = "可继续添加文件，已确认的云端文件会跳过".into();
        cx.notify();
    }

    pub(super) fn start_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.continue_action(cx);
    }

    pub(super) fn continue_action(&mut self, cx: &mut Context<Self>) {
        if self.busy() || self.screen() != Screen::Review {
            return;
        }
        let target = match self.backup_album_target(cx) {
            Ok(target) => target,
            Err(error) => {
                self.notice = error;
                cx.notify();
                return;
            }
        };
        let files = self.queue.files.clone();
        self.backup = BackupTask::new(&files);
        self.launch_upload(files, target, cx);
    }

    fn continue_files(&mut self, files: Vec<MediaFile>, cx: &mut Context<Self>) {
        if self.busy() || self.account.problem.is_some() {
            return;
        }
        let target = match self.backup_album_target(cx) {
            Ok(target) => target,
            Err(error) => {
                self.notice = error;
                cx.notify();
                return;
            }
        };
        self.launch_upload(files, target, cx);
    }

    fn launch_upload(
        &mut self,
        files: Vec<MediaFile>,
        target: AlbumTarget,
        cx: &mut Context<Self>,
    ) {
        let Some(credentials) = self.account.credentials.clone() else {
            self.account.auth_open = true;
            cx.notify();
            return;
        };
        let mut album = std::mem::take(&mut self.backup.album);
        album.target = target;
        self.backup.progress = BackupProgress {
            total: files.len(),
            ..BackupProgress::default()
        };
        self.backup.state = TaskState::Running;
        self.backup.error = None;
        self.backup.cancelled = false;
        self.backup.filter = ResultFilter::All;
        self.backup.page = 0;
        self.thumbnails.clear();
        self.undo = None;
        self.notice = "正在检查云端并处理备份队列…".into();
        let cancel = self.backup.progress.cancel.clone();
        let paused = self.backup.progress.paused.clone();
        let threads = self.upload_threads;
        cx.notify();

        let (sender, receiver) = async_channel::bounded(128);
        cx.background_spawn(async move {
            let result = backup::run(files, credentials, cancel, paused, &sender, threads, album);
            let _ = sender.send_blocking(Event::Finished(result));
        })
        .detach();
        cx.spawn(async move |this, cx| {
            while let Ok(event) = receiver.recv().await {
                let finished = matches!(&event, Event::Finished(_));
                let _ = this.update(cx, |app, cx| {
                    app.apply_backup_event(event, cx);
                    cx.notify();
                });
                if finished {
                    break;
                }
            }
        })
        .detach();
    }

    fn apply_backup_event(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Paused(generation) => {
                if self.backup.state == TaskState::Pausing(generation) {
                    self.backup.state = TaskState::Paused;
                    self.notice = "已暂停读取，已发送的数据仍可能在传输".into();
                }
            }
            Event::Stage { path, phase } => {
                self.backup.progress.transfers.insert(
                    path.clone(),
                    TransferProgress {
                        phase,
                        sent: 0,
                        total: 0,
                    },
                );
                self.request_thumbnail(path, cx);
                if self.backup.state == TaskState::Running {
                    self.notice.clear();
                }
            }
            Event::Transfer { path, sent, total } => {
                if let Some(progress) = self.backup.progress.transfers.get_mut(&path) {
                    progress.sent = sent;
                    progress.total = total;
                }
            }
            Event::Item { path, outcome } => {
                self.backup.progress.transfers.remove(&path);
                self.thumbnails.remove(&path);
                self.backup.outcomes.insert(path, outcome);
                self.backup.progress.processed += 1;
            }
            Event::Finished(result) => {
                self.backup.state = TaskState::Done;
                self.backup.progress.transfers.clear();
                match result {
                    Ok(Report {
                        cancelled,
                        stopped,
                        album,
                    }) => {
                        self.backup.cancelled = cancelled;
                        if let Some(error) = stopped.as_ref().or(album.error.as_ref()) {
                            self.record_authorization_problem(error);
                        }
                        self.notice = if cancelled {
                            "上传已停止；已完成的文件不会撤回".into()
                        } else if let Some(error) = &stopped {
                            format!("任务提前结束：{error}")
                        } else if album.has_pending() {
                            "文件处理已结束，相册整理尚未完成".into()
                        } else if self.backup.counts().needs_processing() > 0 {
                            "部分文件需要继续处理".into()
                        } else {
                            "本次备份处理完成，可在 Google Photos 核验文件与配额".into()
                        };
                        self.backup.error = stopped;
                        self.backup.album = album;
                    }
                    Err(error) => {
                        self.record_authorization_problem(&error);
                        self.notice = format!("无法继续备份：{error}");
                        self.backup.error = Some(error);
                    }
                }
                self.request_details_thumbnails(cx);
            }
        }
    }

    pub(super) fn stop_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.stop_upload(cx);
    }

    pub(super) fn stop_hotkey(&mut self, _: &StopBackup, _: &mut Window, cx: &mut Context<Self>) {
        self.stop_upload(cx);
    }

    fn stop_upload(&mut self, cx: &mut Context<Self>) {
        if !matches!(
            self.backup.state,
            TaskState::Running | TaskState::Pausing(_) | TaskState::Paused
        ) {
            return;
        }
        self.backup.progress.cancel.store(true, Ordering::Relaxed);
        self.backup.state = TaskState::Stopping;
        self.notice = "正在停止当前操作；已完成的备份将保留".into();
        cx.notify();
    }
}
