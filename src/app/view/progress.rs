use super::*;

impl UploadApp {
    pub(super) fn uploading(&self, cx: &App) -> gpui_kit::Div {
        let mut transfers: Vec<_> = self.backup.progress.transfers.iter().collect();
        transfers.sort_by_key(|(path, _)| *path);
        let overall = self.backup.progress.processed as f32 / self.backup.progress.total as f32;
        let counts = self.backup.counts();
        let status = match self.backup.state {
            TaskState::Running => "正在备份",
            TaskState::Pausing(_) => "正在暂停…",
            TaskState::Paused => "已暂停",
            TaskState::Stopping => "正在停止…",
            TaskState::Idle | TaskState::OrganizingAlbum | TaskState::Done => {
                unreachable!("备份页面缺少运行状态")
            }
        };
        card(cx)
            .p_8()
            .gap_5()
            .child(
                div()
                    .text_xs()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .text_color(cx.theme().muted_foreground)
                    .child("文件处理进度"),
            )
            .child(
                div()
                    .flex()
                    .items_end()
                    .justify_between()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_2xl()
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .child(status),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!(
                                        "已处理 {} / {} 项",
                                        self.backup.progress.processed, self.backup.progress.total
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child(format!("{:.0}%", overall * 100.0)),
                    ),
            )
            .child(progress_bar(overall))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "已上传 {} · 已存在 {} · 需处理 {}",
                        counts.uploaded,
                        counts.exists,
                        counts.failed + counts.uncertain,
                    )),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("处理数包括已存在、失败和待核实的文件。"),
            )
            .child(div().h(px(1.0)).bg(cx.theme().border))
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(if transfers.is_empty() {
                        "正在连接…".to_owned()
                    } else {
                        format!("正在处理 {} 项", transfers.len())
                    }),
            )
            .children(transfers.into_iter().map(|(path, progress)| {
                let TransferProgress { phase, sent, total } = progress;
                let description = if *total > 0 && matches!(phase, Phase::Uploading) {
                    format!(
                        "{} · {} / {}",
                        phase.label(),
                        format_size(*sent),
                        format_size(*total)
                    )
                } else {
                    phase.label().to_owned()
                };
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(thumbnail(cx, path, self.thumbnails.get(path)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div().text_sm().child(
                                    path.file_name()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                        .into_owned(),
                                ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(description),
                            ),
                    )
            }))
    }
}
