use super::*;
use crate::{backup::Event, thumbnail};
use std::sync::atomic::Ordering;

impl UploadApp {
    pub(super) fn details_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.show_details = true;
        self.result_filter = ResultFilter::All;
        self.result_page = 0;
        self.request_details_thumbnails(cx);
        cx.notify();
    }

    pub(super) fn filter_results(&mut self, filter: ResultFilter, cx: &mut Context<Self>) {
        self.result_filter = filter;
        self.result_page = 0;
        self.request_details_thumbnails(cx);
        cx.notify();
    }

    fn request_thumbnail(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.thumbnails.contains_key(&path) {
            return;
        }
        self.thumbnail_sequence = self.thumbnail_sequence.wrapping_add(1);
        let sequence = self.thumbnail_sequence;
        self.thumbnails
            .insert(path.clone(), Thumbnail::Loading(sequence));
        cx.spawn(async move |this, cx| {
            let source = path.clone();
            let result = cx.background_spawn(async move { thumbnail::load(&source) }).await;
            let _ = this.update(cx, |app, cx| {
                if matches!(app.thumbnails.get(&path), Some(Thumbnail::Loading(current)) if *current == sequence) {
                    app.thumbnails.insert(path, match result {
                        Ok(image) => Thumbnail::Ready(image),
                        Err(_) => Thumbnail::Unavailable,
                    });
                    cx.notify();
                }
            });
        }).detach();
    }

    fn request_details_thumbnails(&mut self, cx: &mut Context<Self>) {
        let paths: Vec<_> = self
            .queue
            .files
            .iter()
            .filter(|file| {
                self.outcomes
                    .get(&file.path)
                    .is_some_and(|outcome| self.result_filter.matches(outcome))
            })
            .skip(self.result_page * PAGE_SIZE)
            .take(PAGE_SIZE)
            .map(|file| file.path.clone())
            .collect();
        self.thumbnails.retain(|path, _| paths.contains(path));
        for path in paths {
            self.request_thumbnail(path, cx);
        }
    }

    pub(super) fn result_page(&mut self, next: bool, cx: &mut Context<Self>) {
        if next {
            self.result_page += 1;
        } else {
            self.result_page = self.result_page.saturating_sub(1);
        }
        self.request_details_thumbnails(cx);
        cx.notify();
    }

    pub(super) fn toggle_pause(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        match self.task {
            TaskState::Running => {
                self.task = TaskState::Pausing(self.paused.request());
                self.notice = "正在暂停，当前网络操作完成后生效".into();
            }
            TaskState::Pausing(_) | TaskState::Paused(_) => {
                self.paused.resume();
                self.task = TaskState::Running;
                self.notice = "已请求继续备份".into();
            }
            TaskState::Idle | TaskState::Stopping | TaskState::Done => return,
        }
        cx.notify();
    }

    pub(super) fn details_back(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.show_details = false;
        cx.notify();
    }

    pub(super) fn continue_remaining(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy() {
            return;
        }
        let files: Vec<_> = self
            .queue
            .files
            .iter()
            .filter(|f| {
                matches!(
                    self.outcomes.get(&f.path),
                    Some(ItemOutcome::Pending | ItemOutcome::Failed(_) | ItemOutcome::Uncertain(_))
                )
            })
            .cloned()
            .collect();
        if !files.is_empty() {
            self.launch_upload(files, cx);
        } else {
            self.notice = "没有需要继续处理的文件".into();
            cx.notify();
        }
    }

    pub(super) fn retry_file(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        if let Some(file) = self
            .queue
            .files
            .iter()
            .find(|file| file.path == path)
            .cloned()
        {
            self.launch_upload(vec![file], cx);
        } else {
            self.notice = "队列中找不到该文件，无法重试".into();
            cx.notify();
        }
    }

    pub(super) fn next_page(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if (self.page + 1) * PAGE_SIZE < self.queue.files.len() {
            self.page += 1;
            cx.notify();
        }
    }

    pub(super) fn previous_page(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.page = self.page.saturating_sub(1);
        cx.notify();
    }

    pub(super) fn return_to_review(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.task = TaskState::Idle;
        self.errors.clear();
        self.show_details = false;
        self.notice = "可继续添加文件，已确认的云端文件会跳过".into();
        cx.notify();
    }

    pub(super) fn reset_progress(&mut self) {
        self.current.clear();
        self.current_path = None;
        self.phase = None;
        self.processed = 0;
        self.transfer = (0, 0);
        self.transfers.clear();
        self.was_cancelled = false;
    }

    pub(super) fn result_counts(&self) -> ResultCounts {
        let mut counts = ResultCounts::default();
        for outcome in self.outcomes.values() {
            match outcome {
                ItemOutcome::Uploaded => counts.uploaded += 1,
                ItemOutcome::Exists => counts.exists += 1,
                ItemOutcome::Failed(_) => counts.failed += 1,
                ItemOutcome::Uncertain(_) => counts.uncertain += 1,
                ItemOutcome::Pending => counts.pending += 1,
            }
        }
        counts
    }

    pub(super) fn start_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.continue_action(cx);
    }

    pub(super) fn continue_action(&mut self, cx: &mut Context<Self>) {
        if self.busy() || self.screen() != Screen::Review {
            return;
        }
        self.launch_upload(self.queue.files.clone(), cx);
    }

    fn launch_upload(&mut self, files: Vec<MediaFile>, cx: &mut Context<Self>) {
        if self.busy() || files.is_empty() {
            return;
        }
        let album = match self.album_target.clone() {
            Some(target) => target,
            None => match backup::parse_album(&self.album_text) {
                Ok(target) => target,
                Err(error) => {
                    self.album_error = Some(error);
                    cx.notify();
                    return;
                }
            },
        };
        self.album_error = None;
        let threads = self.upload_threads;
        let Some(credentials) = self.credentials.clone() else {
            self.auth_open = true;
            cx.notify();
            return;
        };
        if self.task != TaskState::Done {
            self.outcomes = files
                .iter()
                .map(|file| (file.path.clone(), ItemOutcome::Pending))
                .collect();
            self.result_bytes = files.iter().map(|file| file.size).sum();
        }
        self.run_total = files.len();
        self.show_details = false;
        self.cancel = Arc::new(AtomicBool::new(false));
        self.paused = Arc::new(PauseControl::default());
        let cancel = self.cancel.clone();
        let paused = self.paused.clone();
        self.reset_progress();
        self.undo = None;
        self.task = TaskState::Running;
        self.task_error = None;
        self.errors.clear();
        self.notice = "正在检查云端并处理备份队列…".into();
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
                    match event {
                        Event::Paused(generation) => {
                            if TaskState::Pausing(generation) == app.task {
                                app.task = TaskState::Paused(generation);
                                app.notice = "已暂停读取，已发送的数据仍可能在传输".into();
                            }
                        }
                        Event::Stage { path, name, phase } => {
                            app.transfers.insert(path.clone(), (phase, 0, 0));
                            app.current = name;
                            app.thumbnails.retain(|cached, _| cached == &path);
                            app.request_thumbnail(path.clone(), cx);
                            app.current_path = Some(path);
                            app.phase = Some(phase);
                            app.transfer = (0, 0);
                            if app.task == TaskState::Running {
                                app.notice.clear();
                            }
                        }
                        Event::Transfer { path, sent, total } => {
                            if let Some(progress) = app.transfers.get_mut(&path) {
                                progress.1 = sent;
                                progress.2 = total;
                            }
                            if app.current_path.as_ref() == Some(&path) {
                                app.transfer = (sent, total);
                            }
                        }
                        Event::Item { path, outcome } => {
                            app.transfers.remove(&path);
                            app.outcomes.insert(path.clone(), outcome);
                            app.processed += 1;
                            if app.current_path.as_ref() == Some(&path) {
                                let next = app
                                    .transfers
                                    .iter()
                                    .next()
                                    .map(|(path, progress)| (path.clone(), *progress));
                                if let Some((next_path, (phase, sent, total))) = next {
                                    app.current = next_path
                                        .file_name()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                        .into_owned();
                                    app.current_path = Some(next_path);
                                    app.phase = Some(phase);
                                    app.transfer = (sent, total);
                                } else {
                                    app.current.clear();
                                    app.current_path = None;
                                    app.phase = None;
                                    app.transfer = (0, 0);
                                }
                            }
                        }
                        Event::AlbumCreated(key) => {
                            app.album_target = Some(backup::AlbumTarget::Existing(key));
                        }
                        Event::Finished(Ok(report)) => {
                            app.task = TaskState::Done;
                            app.was_cancelled = report.cancelled;
                            app.errors = report.errors;
                            app.transfer = (0, 0);
                            app.task_error = report.album_error.clone();
                            let counts = app.result_counts();
                            if let Some(reason) = report.stopped.clone() {
                                app.task_error = Some(reason);
                            }
                            app.notice = if report.cancelled {
                                "上传已停止；已完成的文件不会撤回".into()
                            } else if report.album_error.is_some() {
                                "文件备份完成，但相册整理失败；原始文件无需重传".into()
                            } else if let Some(reason) = &report.stopped {
                                let problem = classify_problem(reason.to_owned());
                                if matches!(problem.kind, ProblemKind::Authorization) {
                                    app.problem = Some(problem);
                                }
                                format!("任务提前结束：{reason}")
                            } else if counts.failed + counts.uncertain + counts.pending > 0 {
                                "部分文件需要继续处理，可查看详情".into()
                            } else {
                                "本次备份处理完成，可在 Google Photos 核验文件与配额".into()
                            };
                        }
                        Event::Finished(Err(error)) => {
                            app.task = TaskState::Done;
                            app.notice = format!("无法继续备份：{error}");
                            app.task_error = Some(error);
                        }
                    }
                    cx.notify();
                });
                if finished {
                    break;
                }
            }
        })
        .detach();
    }

    pub(super) fn stop_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.stop_upload(cx);
    }

    fn stop_upload(&mut self, cx: &mut Context<Self>) {
        if !self.task.is_running() || self.task == TaskState::Stopping {
            return;
        }
        self.cancel.store(true, Ordering::Relaxed);
        self.task = TaskState::Stopping;
        self.notice = "正在停止当前操作；已完成的备份将保留".into();
        cx.notify();
    }
}
