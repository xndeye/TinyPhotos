use super::*;
use gpui_kit::component::{Sizable, alert::Alert, input::Input};

impl UploadApp {
    pub(super) fn results(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let counts = self.backup.counts();
        let total = self.backup.outcomes.len();
        let needs_attention = counts.needs_processing();
        let needs_album_verification = self.backup.album.needs_verification();
        let organizing_album = self.backup.state == TaskState::OrganizingAlbum;
        div().w_full().flex().flex_col().gap_5().child(card(cx)
            .p_8()
            .gap_5()
            .child(
                div()
                    .text_xs()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .text_color(cx.theme().muted_foreground)
                    .child("备份结果"),
            )
            .child(
                div()
                    .text_2xl()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(if organizing_album {
                        "正在整理相册"
                    } else if self.backup.cancelled {
                        "备份已停止"
                    } else if needs_attention > 0 || self.backup.error.is_some() || self.backup.album.has_pending() {
                        "备份未完成"
                    } else {
                        "备份完成"
                    }),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "已处理 {} / {} 个文件 · 所选文件共 {}",
                        total - counts.pending,
                        total,
                        format_size(self.queue.total_bytes())
                    )),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_wrap()
                    .gap_3()
                    .child(result_metric(cx, "已上传", counts.uploaded, cx.theme().success))
                    .child(result_metric(cx, "已存在", counts.exists, cx.theme().muted_foreground))
                    .child(result_metric(cx, "失败", counts.failed, cx.theme().danger))
                    .child(result_metric(cx, "待核实", counts.uncertain, cx.theme().danger))
                    .child(result_metric(cx, "未处理", counts.pending, cx.theme().muted_foreground)),
            )
            .when_some(self.backup.error.as_ref(), |view, error| {
                view.child(Alert::error("task-error", error.to_string()).title("任务提前结束"))
            })
            .when(self.backup.album.has_pending() || organizing_album, |view| {
                view.when_some(self.backup.album.error.as_ref(), |view, error| {
                    view.child(Alert::error("album-error", error.to_string()).title("相册整理未完成"))
                })
                .when(self.backup.album.error.is_none(), |view| {
                    view.child(div().text_sm().child(if organizing_album {
                        "正在整理相册…"
                    } else {
                        "已备份的文件尚未全部加入目标相册。"
                    }))
                })
                .when(needs_album_verification, |view| {
                    view.child(div().text_sm().child("请先在 Google Photos 核验相册，再填写现有相册链接或 ID，避免重复创建。"))
                        .child(div().text_sm().child("已核验的目标相册"))
                        .child(Input::new(&self.album_input).disabled(self.busy()))
                        .when_some(backup::parse_album(self.album_input.read(cx).value().as_ref()).err(), |view, error| {
                            view.child(div().text_sm().text_color(cx.theme().danger).child(error))
                        })
                })
                .child(Button::new("retry-album").label("整理相册").disabled(self.busy() || self.account.problem.is_some() || self.backup_album_target(cx).is_err()).on_click(cx.listener(Self::retry_album)))
            })
            .when(self.account.problem.is_some(), |view| {
                view.child(Button::new("results-reauthorize").label("重新授权…").disabled(self.busy()).on_click(cx.listener(Self::reauthorize)))
            })
            .when(counts.uncertain > 0, |view| {
                view.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("待核实表示服务端结果尚未确认；继续处理时会先检查云端。"),
                )
            })
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("请在 Google Photos 中核验备份。"),
            )).child(self.details(&counts, cx))
    }
    fn details(&self, counts: &ResultCounts, cx: &mut Context<Self>) -> gpui_kit::Div {
        let items: Vec<_> = self
            .queue
            .files
            .iter()
            .filter_map(|file| {
                self.backup
                    .outcomes
                    .get(&file.path)
                    .map(|outcome| (file, outcome))
            })
            .filter(|(_, outcome)| self.backup.filter.matches(outcome))
            .collect();
        let pages = items.len().div_ceil(PAGE_SIZE);
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div().flex().items_center().gap_3().child(
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
                            format!("全部 {}", self.backup.outcomes.len()),
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
                            .toggled(self.backup.filter == filter)
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
                        .text_color(cx.theme().muted_foreground)
                        .child("此分类没有文件"),
                )
            })
            .child(
                div().w_full().flex().flex_col().gap_2().children(
                    items
                        .into_iter()
                        .skip(self.backup.page * PAGE_SIZE)
                        .take(PAGE_SIZE)
                        .map(|(file, outcome)| {
                            let path = &file.path;
                            let message = match outcome {
                                ItemOutcome::Pending => "尚未处理".to_owned(),
                                ItemOutcome::Uploaded => "已上传".to_owned(),
                                ItemOutcome::Exists => "云端已存在".to_owned(),
                                ItemOutcome::Failed(error) => format!("失败：{error}"),
                                ItemOutcome::Uncertain(error) => format!("待核实：{error}"),
                            };
                            let retryable = outcome.needs_processing();
                            div()
                                .w_full()
                                .border_b_1()
                                .border_color(cx.theme().border)
                                .py_3()
                                .flex()
                                .flex_row()
                                .flex_wrap()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(thumbnail(cx, path, self.thumbnails.get(path)))
                                .child(
                                    div()
                                        .flex_1()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .min_w(px(0.0))
                                        .child(
                                            file_text(
                                                format!("result-name-{}", path.display()),
                                                file.name.clone(),
                                            )
                                            .text_sm(),
                                        )
                                        .child(
                                            file_text(
                                                format!("result-path-{}", path.display()),
                                                path.to_string_lossy().into_owned(),
                                            )
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(if retryable {
                                                    cx.theme().danger
                                                } else {
                                                    cx.theme().muted_foreground
                                                })
                                                .child(message),
                                        ),
                                )
                                .when(retryable, |row| {
                                    let retry_file = file.clone();
                                    row.child(
                                        Button::new(format!("retry-{}", path.display()))
                                            .label("检查并处理")
                                            .ghost()
                                            .disabled(self.busy() || self.account.problem.is_some())
                                            .accessibility_label(format!(
                                                "检查并处理 {}",
                                                path.display()
                                            ))
                                            .on_click(cx.listener(
                                                move |app, _: &ClickEvent, _, cx| {
                                                    app.retry_file(retry_file.clone(), cx)
                                                },
                                            )),
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
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("{} / {} 页", self.backup.page + 1, pages)),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .when(self.backup.page > 0, |row| {
                                    row.child(
                                        Button::new("result-prev")
                                            .label("上一页")
                                            .ghost()
                                            .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                                                app.set_result_page(app.backup.page - 1, cx)
                                            })),
                                    )
                                })
                                .when(self.backup.page + 1 < pages, |row| {
                                    row.child(
                                        Button::new("result-next")
                                            .label("下一页")
                                            .ghost()
                                            .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                                                app.set_result_page(app.backup.page + 1, cx)
                                            })),
                                    )
                                }),
                        ),
                )
            })
    }
}
