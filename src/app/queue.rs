use super::*;

impl UploadApp {
    pub(super) fn open_files(&mut self, _: &OpenFiles, _: &mut Window, cx: &mut Context<Self>) {
        self.choose(false, cx);
    }

    pub(super) fn open_folders(&mut self, _: &OpenFolders, _: &mut Window, cx: &mut Context<Self>) {
        self.choose(true, cx);
    }

    pub(super) fn choose_files(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.choose(false, cx);
    }

    pub(super) fn choose_folders(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.choose(true, cx);
    }

    fn choose(&mut self, folders: bool, cx: &mut Context<Self>) {
        if self.busy() || !matches!(self.screen(), Screen::Empty | Screen::Review) {
            return;
        }
        self.scanning = true;
        self.notice = "正在选择来源…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let selected = cx
                .background_spawn(async move {
                    let dialog = rfd::FileDialog::new().set_title(if folders {
                        "选择需要备份的文件夹"
                    } else {
                        "选择需要备份的照片和视频"
                    });
                    if folders {
                        dialog.pick_folders()
                    } else {
                        dialog.pick_files()
                    }
                })
                .await;
            let _ = this.update(cx, |app, cx| {
                // Release the dialog guard before starting the scan.
                app.scanning = false;
                if let Some(paths) = selected {
                    app.add_paths(paths, cx);
                } else {
                    app.notice = "已取消选择".into();
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn drop_paths(
        &mut self,
        paths: &ExternalPaths,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.add_paths(paths.paths().to_vec(), cx);
    }

    fn add_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if self.busy() || paths.is_empty() {
            return;
        }
        self.scanning = true;
        self.notice = format!("正在扫描 {} 个来源…", paths.len());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_spawn(async move { crate::queue::scan(&paths) })
                .await;
            let _ = this.update(cx, |app, cx| {
                app.scanning = false;
                let (added, ignored, warnings) = app.queue.merge(found);
                app.thumbnails.clear();
                if added > 0 {
                    app.backup = BackupTask::default();
                    app.undo = None;
                }
                app.page = 0;
                app.queue_errors.clear();
                app.notice = if added > 0 {
                    format!("已加入 {added} 项")
                } else {
                    "没有新增文件".into()
                };
                if ignored > 0 {
                    app.notice.push_str(&format!("，忽略 {ignored} 项"));
                }
                if !warnings.is_empty() {
                    app.notice.push_str(" · 存在扫描问题");
                    app.queue_errors = warnings;
                }
                app.persist_queue();
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn remove_file(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        let file = self.queue.remove(index);
        self.undo = Some(UndoChange::File(index, file));
        self.backup = BackupTask::default();
        self.page = self
            .page
            .min(self.queue.files.len().saturating_sub(1) / PAGE_SIZE);
        self.notice = "已从队列移除；可撤销，本地文件不受影响".into();
        self.persist_queue();
        cx.notify();
    }

    pub(super) fn undo_remove(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        if let Some(change) = self.undo.take() {
            match change {
                UndoChange::File(index, file) => {
                    self.queue.restore(index, file);
                    self.page = index / PAGE_SIZE;
                }
                UndoChange::Queue(queue) => self.queue = queue,
            }
            self.backup = BackupTask::default();
            self.notice = "已恢复队列".into();
            self.persist_queue();
            cx.notify();
        }
    }

    pub(super) fn clear_queue(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        self.undo = Some(UndoChange::Queue(std::mem::take(&mut self.queue)));
        self.thumbnails.clear();
        self.backup = BackupTask::default();
        self.page = 0;
        self.queue_errors.clear();
        self.notice = "队列已清空；可撤销，本地原件保持不变".into();
        self.persist_queue();
        cx.notify();
    }

    fn persist_queue(&mut self) {
        let paths: Vec<_> = self.queue.files.iter().map(|f| f.path.clone()).collect();
        if let Err(error) = storage::save_queue(&paths) {
            self.notice = format!("队列暂未持久保存：{error}");
        }
    }
    pub(super) fn next_page(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.page += 1;
        cx.notify();
    }

    pub(super) fn previous_page(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.page -= 1;
        cx.notify();
    }
}
