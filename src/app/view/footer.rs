use super::*;

impl UploadApp {
    pub(super) fn footer(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let screen = self.screen();
        let pause_shortcut = if cfg!(target_os = "macos") {
            "暂停或继续 · ⌘⇧P"
        } else {
            "暂停或继续 · Ctrl+Shift+P"
        };
        let stop_shortcut = if cfg!(target_os = "macos") {
            "停止备份 · ⌘⇧S"
        } else {
            "停止备份 · Ctrl+Shift+S"
        };
        let status = if self.notice.is_empty() && screen == Screen::Review {
            format!(
                "{} 个文件 · {}",
                self.queue.files.len(),
                format_size(self.queue.total_bytes())
            )
        } else {
            self.notice.clone()
        };
        let retryable = screen == Screen::Results
            && self
                .backup
                .outcomes
                .values()
                .any(ItemOutcome::needs_processing);
        div()
            .w_full()
            .bg(cx.theme().background)
            .border_t_1()
            .border_color(cx.theme().border)
            .px_8()
            .py_3()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap_4()
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(status),
            )
            .when(
                self.undo.is_some() && matches!(screen, Screen::Empty | Screen::Review),
                |row| {
                    row.child(
                        Button::new("undo")
                            .label("撤销")
                            .ghost()
                            .disabled(self.busy())
                            .on_click(cx.listener(Self::undo_remove)),
                    )
                },
            )
            .when(screen == Screen::Review, |row| {
                row.child(
                    Button::new("start")
                        .label("开始备份")
                        .primary()
                        .disabled(self.busy() || self.backup_album_target(cx).is_err())
                        .on_click(cx.listener(Self::start_click)),
                )
            })
            .when(screen == Screen::Uploading, |row| {
                row.child(
                    Button::new("pause")
                        .label(match self.backup.state {
                            TaskState::Running => "暂停",
                            TaskState::Pausing(_) => "取消暂停",
                            TaskState::Paused => "继续",
                            TaskState::Stopping => "暂停",
                            TaskState::Idle | TaskState::OrganizingAlbum | TaskState::Done => {
                                unreachable!("备份页面缺少运行状态")
                            }
                        })
                        .disabled(self.backup.state == TaskState::Stopping)
                        .tooltip(pause_shortcut)
                        .on_click(cx.listener(Self::toggle_pause)),
                )
                .child(
                    Button::new("stop")
                        .label("停止")
                        .disabled(self.backup.state == TaskState::Stopping)
                        .tooltip(stop_shortcut)
                        .on_click(cx.listener(Self::stop_click)),
                )
            })
            .when(screen == Screen::Results, |row| {
                row.when(retryable && self.account.problem.is_none(), |row| {
                    row.child(
                        Button::new("retry")
                            .label("继续处理")
                            .primary()
                            .disabled(self.busy() || self.backup_album_target(cx).is_err())
                            .on_click(cx.listener(Self::continue_remaining)),
                    )
                })
                .child(
                    Button::new("review")
                        .label("返回清单")
                        .when(!retryable, |button| button.primary())
                        .disabled(self.busy())
                        .on_click(cx.listener(Self::return_to_review)),
                )
            })
    }
}
