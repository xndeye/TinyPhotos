use super::*;
use gpui_kit::component::{Sizable, alert::Alert};

impl UploadApp {
    pub(super) fn uploading(&self) -> gpui_kit::Div {
        let (sent, total) = self.transfer;
        let overall = if self.run_total > 0 {
            self.processed as f32 / self.run_total as f32
        } else {
            0.0
        };
        let status = match self.task {
            TaskState::Running => "正在备份",
            TaskState::Pausing(_) => "正在暂停…",
            TaskState::Paused(_) => "已暂停",
            TaskState::Stopping => "正在停止…",
            TaskState::Idle | TaskState::Done => unreachable!("备份页面缺少运行状态"),
        };
        card(self.dark)
            .p_8()
            .gap_5()
            .child(
                div()
                    .text_xs()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .text_color(rgb(theme(self.dark, MUTED)))
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
                                    .text_color(rgb(theme(self.dark, MUTED)))
                                    .child(format!(
                                        "已处理 {} / {} 项",
                                        self.processed, self.run_total
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
                    .text_xs()
                    .text_color(rgb(theme(self.dark, MUTED)))
                    .child("处理数包括已存在、失败和待核实的文件。"),
            )
            .child(div().h(px(1.0)).bg(rgb(theme(self.dark, HAIRLINE))))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(thumbnail(
                        self.dark,
                        self.current_path.as_deref(),
                        self.current_path
                            .as_ref()
                            .and_then(|path| self.thumbnails.get(path)),
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .child(if self.current.is_empty() {
                                        "正在连接…".to_owned()
                                    } else {
                                        self.current.clone()
                                    }),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(theme(self.dark, MUTED)))
                                    .child(
                                        if total > 0 && matches!(self.phase, Some(Phase::Uploading))
                                        {
                                            format!(
                                                "传输原始文件 · 当前文件 {} / {}",
                                                format_size(sent),
                                                format_size(total)
                                            )
                                        } else {
                                            self.phase.map_or("", Phase::label).to_owned()
                                        },
                                    ),
                            ),
                    ),
            )
    }
    pub(super) fn results(&self) -> gpui_kit::Div {
        let counts = self.result_counts();
        let total = self.outcomes.len();
        let needs_attention = counts.failed + counts.uncertain + counts.pending;
        card(self.dark)
            .p_8()
            .gap_5()
            .child(
                div()
                    .text_xs()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .text_color(rgb(theme(self.dark, MUTED)))
                    .child("备份结果"),
            )
            .child(
                div()
                    .text_2xl()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(if self.was_cancelled {
                        "备份已停止"
                    } else if needs_attention > 0 || self.task_error.is_some() {
                        "备份未完成"
                    } else {
                        "备份完成"
                    }),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(theme(self.dark, MUTED)))
                    .child(format!(
                        "已处理 {} / {} 个文件 · 所选文件共 {}",
                        total - counts.pending,
                        total,
                        format_size(self.result_bytes)
                    )),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_wrap()
                    .gap_3()
                    .child(result_metric(self.dark, "已上传", counts.uploaded, SUCCESS))
                    .child(result_metric(self.dark, "已存在", counts.exists, MUTED))
                    .child(result_metric(self.dark, "失败", counts.failed, DANGER))
                    .child(result_metric(self.dark, "待核实", counts.uncertain, DANGER))
                    .child(result_metric(self.dark, "未处理", counts.pending, MUTED)),
            )
            .when_some(self.task_error.clone(), |view, error| {
                view.child(Alert::error("task-error", error).title("任务提前结束"))
            })
            .when(counts.uncertain > 0, |view| {
                view.child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme(self.dark, MUTED)))
                        .child("待核实表示服务端结果尚未确认；继续处理时会先检查云端。"),
                )
            })
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(theme(self.dark, MUTED)))
                    .child("请在 Google Photos 中核验备份。"),
            )
    }
    pub(super) fn details(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let items: Vec<_> = self
            .queue
            .files
            .iter()
            .filter_map(|file| {
                self.outcomes
                    .get(&file.path)
                    .map(|outcome| (&file.path, outcome))
            })
            .filter(|(_, outcome)| self.result_filter.matches(outcome))
            .collect();
        let pages = items.len().div_ceil(PAGE_SIZE);
        let counts = self.result_counts();
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        text_button("back-results", "返回结果")
                            .on_click(cx.listener(Self::details_back)),
                    )
                    .child(
                        div()
                            .text_xl()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child("上传结果"),
                    ),
            )
            .child(
                div().w_full().flex().flex_wrap().gap_2().children(
                    [
                        (
                            ResultFilter::All,
                            "filter-all",
                            format!("全部 {}", self.outcomes.len()),
                        ),
                        (
                            ResultFilter::Uploaded,
                            "filter-uploaded",
                            format!("已上传 {}", counts.uploaded),
                        ),
                        (
                            ResultFilter::Exists,
                            "filter-exists",
                            format!("已存在 {}", counts.exists),
                        ),
                        (
                            ResultFilter::Failed,
                            "filter-failed",
                            format!("失败 {}", counts.failed),
                        ),
                        (
                            ResultFilter::Uncertain,
                            "filter-uncertain",
                            format!("待核实 {}", counts.uncertain),
                        ),
                        (
                            ResultFilter::Pending,
                            "filter-pending",
                            format!("未处理 {}", counts.pending),
                        ),
                    ]
                    .into_iter()
                    .map(|(filter, id, label)| {
                        Button::new(id)
                            .small()
                            .label(label)
                            .toggled(self.result_filter == filter)
                            .when(self.result_filter == filter, |button| button.primary())
                            .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                                app.filter_results(filter, cx)
                            }))
                    }),
                ),
            )
            .when(items.is_empty(), |view| {
                view.child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme(self.dark, MUTED)))
                        .child("此分类没有文件"),
                )
            })
            .child(
                div().w_full().flex().flex_col().gap_2().children(
                    items
                        .into_iter()
                        .skip(self.result_page * PAGE_SIZE)
                        .take(PAGE_SIZE)
                        .map(|(path, outcome)| {
                            let message = match outcome {
                                ItemOutcome::Pending => "尚未处理".to_owned(),
                                ItemOutcome::Uploaded => "已上传".to_owned(),
                                ItemOutcome::Exists => "云端已存在".to_owned(),
                                ItemOutcome::Failed(error) => format!("失败：{error}"),
                                ItemOutcome::Uncertain(error) => format!("待核实：{error}"),
                            };
                            let path = path.clone();
                            let retryable = matches!(
                                outcome,
                                ItemOutcome::Pending
                                    | ItemOutcome::Failed(_)
                                    | ItemOutcome::Uncertain(_)
                            );
                            let retry_path = path.clone();
                            card(self.dark)
                                .p_3()
                                .flex_row()
                                .flex_wrap()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(thumbnail(
                                    self.dark,
                                    Some(&path),
                                    self.thumbnails.get(&path),
                                ))
                                .child(
                                    div()
                                        .flex_1()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .min_w(px(0.0))
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
                                                .text_color(rgb(theme(self.dark, MUTED)))
                                                .child(path.to_string_lossy().into_owned()),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(rgb(theme(
                                                    self.dark,
                                                    if retryable { DANGER } else { MUTED },
                                                )))
                                                .child(message),
                                        ),
                                )
                                .when(retryable, |row| {
                                    row.child(
                                        text_button(
                                            format!("retry-{}", path.display()),
                                            "检查并处理",
                                        )
                                        .accessibility_label(format!(
                                            "检查并处理 {}",
                                            path.display()
                                        ))
                                        .on_click(
                                            cx.listener(move |app, _: &ClickEvent, _, cx| {
                                                app.retry_file(&retry_path, cx)
                                            }),
                                        ),
                                    )
                                })
                        }),
                ),
            )
            .when(pages > 1, |view| {
                view.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(theme(self.dark, MUTED)))
                                .child(format!("{} / {} 页", self.result_page + 1, pages)),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .when(self.result_page > 0, |row| {
                                    row.child(text_button("result-prev", "上一页").on_click(
                                        cx.listener(|app, _: &ClickEvent, _, cx| {
                                            app.result_page(false, cx)
                                        }),
                                    ))
                                })
                                .when(self.result_page + 1 < pages, |row| {
                                    row.child(text_button("result-next", "下一页").on_click(
                                        cx.listener(|app, _: &ClickEvent, _, cx| {
                                            app.result_page(true, cx)
                                        }),
                                    ))
                                }),
                        ),
                )
            })
    }

    pub(super) fn footer(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let screen = self.screen();
        let status = self.notice.clone();
        let retryable = screen == Screen::Results
            && self.outcomes.values().any(|outcome| {
                matches!(
                    outcome,
                    ItemOutcome::Pending | ItemOutcome::Failed(_) | ItemOutcome::Uncertain(_)
                )
            });
        div()
            .w_full()
            .bg(rgb(theme(self.dark, BACKGROUND)))
            .border_t_1()
            .border_color(rgb(theme(self.dark, HAIRLINE)))
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
                    .text_color(rgb(theme(self.dark, MUTED)))
                    .child(status),
            )
            .when(
                self.undo.is_some() && matches!(screen, Screen::Empty | Screen::Review),
                |row| {
                    row.child(
                        text_button("undo", "撤销")
                            .disabled(self.busy())
                            .on_click(cx.listener(Self::undo_remove)),
                    )
                },
            )
            .when(screen == Screen::Review, |row| {
                row.child(
                    button("start", "开始上传", true, !self.busy())
                        .on_click(cx.listener(Self::start_click)),
                )
            })
            .when(screen == Screen::Uploading, |row| {
                row.child(
                    button(
                        "pause",
                        match self.task {
                            TaskState::Running => "暂停",
                            TaskState::Pausing(_) => "取消暂停",
                            TaskState::Paused(_) => "继续",
                            TaskState::Stopping => "暂停",
                            TaskState::Idle | TaskState::Done => {
                                unreachable!("备份页面缺少运行状态")
                            }
                        },
                        false,
                        self.task != TaskState::Stopping,
                    )
                    .on_click(cx.listener(Self::toggle_pause)),
                )
                .child(
                    button("stop", "停止", false, self.task != TaskState::Stopping)
                        .on_click(cx.listener(Self::stop_click)),
                )
            })
            .when(screen == Screen::Results, |row| {
                row.child(
                    text_button("result-details", "查看详情")
                        .on_click(cx.listener(Self::details_click)),
                )
                .when(retryable, |row| {
                    row.child(
                        button("retry", "继续处理", true, true)
                            .on_click(cx.listener(Self::continue_remaining)),
                    )
                })
                .child(
                    button("review", "返回清单", !retryable, true)
                        .on_click(cx.listener(Self::return_to_review)),
                )
            })
    }
}
