use super::*;

impl UploadApp {
    fn source(&self, cx: &mut Context<Self>, empty: bool) -> gpui_kit::Stateful<gpui_kit::Div> {
        let drag_background = cx.theme().muted;
        let drag_border = cx.theme().primary;
        let zone = div()
            .id(if empty { "source-empty" } else { "source-more" })
            .w_full()
            .when(empty, |zone| {
                zone.rounded_xl()
                    .bg(cx.theme().background)
                    .border_1()
                    .border_color(cx.theme().border)
            })
            .when(!self.busy(), |zone| {
                zone.drag_over::<ExternalPaths>(move |style, _, _, _| {
                    style.bg(drag_background).border_color(drag_border)
                })
            })
            .on_drop(cx.listener(Self::drop_paths));
        if empty {
            zone.p_8()
                .flex()
                .flex_col()
                .items_start()
                .gap_4()
                .child(
                    div()
                        .size(px(48.0))
                        .rounded_lg()
                        .bg(cx.theme().muted)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(illustration(Illustration::Upload, 24.0, cx.theme().primary)),
                )
                .child(
                    div()
                        .text_xl()
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .child("添加照片和视频"),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("拖入文件或文件夹，也可以从设备中选择。"),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .pt_2()
                        .child(
                            Button::new("files-empty")
                                .label("选择文件…")
                                .disabled(self.busy())
                                .on_click(cx.listener(Self::choose_files)),
                        )
                        .child(
                            Button::new("folders-empty")
                                .label("选择文件夹…")
                                .disabled(self.busy())
                                .on_click(cx.listener(Self::choose_folders)),
                        ),
                )
        } else {
            zone.py_2()
                .flex()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap_4()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                .child("继续添加"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("也可拖入文件或文件夹"),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            Button::new("files-more")
                                .label("添加文件…")
                                .disabled(self.busy())
                                .on_click(cx.listener(Self::choose_files)),
                        )
                        .child(
                            Button::new("folders-more")
                                .label("添加文件夹…")
                                .disabled(self.busy())
                                .on_click(cx.listener(Self::choose_folders)),
                        ),
                )
        }
    }
    fn files(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let count = self.queue.files.len();
        let pages = count.div_ceil(PAGE_SIZE);
        div()
            .w_full()
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .p_5()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_start()
                            .gap_1()
                            .child(
                                div()
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .child("备份清单"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!(
                                        "{count} 个文件 · {}",
                                        format_size(self.queue.total_bytes())
                                    )),
                            ),
                    )
                    .child(
                        Button::new("clear-queue")
                            .label("清空队列")
                            .ghost()
                            .disabled(self.busy())
                            .on_click(cx.listener(Self::clear_queue)),
                    ),
            )
            .child(div().h(px(1.0)).bg(cx.theme().border))
            .child(
                div()
                    .id("file-list")
                    .max_h(px(320.0))
                    .overflow_y_scroll()
                    .children(
                        self.queue
                            .files
                            .iter()
                            .enumerate()
                            .skip(self.page * PAGE_SIZE)
                            .take(PAGE_SIZE)
                            .map(|(index, item)| {
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .py_2()
                                    .child(
                                        div()
                                            .size(px(36.0))
                                            .rounded_md()
                                            .bg(cx.theme().muted)
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(file_icon(&item.path)),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.0))
                                            .flex()
                                            .flex_col()
                                            .gap_1()
                                            .child(
                                                file_text(
                                                    format!("file-name-{}", item.path.display()),
                                                    item.name.clone(),
                                                )
                                                .text_sm(),
                                            )
                                            .child(
                                                file_text(
                                                    format!("file-path-{}", item.path.display()),
                                                    item.path.to_string_lossy().into_owned(),
                                                )
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(format_size(item.size)),
                                    )
                                    .child(
                                        Button::new(format!("remove-{}", item.path.display()))
                                            .label("移除")
                                            .ghost()
                                            .disabled(self.busy())
                                            .accessibility_label(format!(
                                                "从队列移除 {}",
                                                item.name
                                            ))
                                            .on_click(cx.listener(
                                                move |app, _: &ClickEvent, _, cx| {
                                                    app.remove_file(index, cx);
                                                },
                                            )),
                                    )
                            }),
                    ),
            )
            .when(pages > 1, |view| {
                view.child(div().h(px(1.0)).bg(cx.theme().border)).child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("{}/{} 页", self.page + 1, pages)),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .when(self.page > 0, |row| {
                                    row.child(
                                        Button::new("prev")
                                            .label("上一页")
                                            .ghost()
                                            .on_click(cx.listener(Self::previous_page)),
                                    )
                                })
                                .when(self.page + 1 < pages, |row| {
                                    row.child(
                                        Button::new("next")
                                            .label("下一页")
                                            .ghost()
                                            .on_click(cx.listener(Self::next_page)),
                                    )
                                }),
                        ),
                )
            })
            .when(!self.queue_errors.is_empty(), |view| {
                view.child(div().h(px(1.0)).bg(cx.theme().border)).children(
                    self.queue_errors.iter().map(|error| {
                        div()
                            .text_xs()
                            .text_color(cx.theme().danger)
                            .child(error.clone())
                    }),
                )
            })
    }

    pub(super) fn review(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let album = self.backup_album_target(cx);
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .text_2xl()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child("准备备份"),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "{} · {} 个文件 · {}",
                        self.account
                            .credentials
                            .as_ref()
                            .map_or("未连接账号", |account| account.email.as_str()),
                        self.queue.files.len(),
                        format_size(self.queue.total_bytes()),
                    )),
            )
            .child(
                card(cx)
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child("目标相册（可选）"),
                    )
                    .child(gpui_kit::component::input::Input::new(&self.album_input))
                    .child(
                        div()
                            .text_xs()
                            .text_color(if album.is_err() {
                                cx.theme().danger
                            } else {
                                cx.theme().muted_foreground
                            })
                            .child(match album {
                                Ok(backup::AlbumTarget::None) => {
                                    "仅备份文件，不加入相册".to_owned()
                                }
                                Ok(backup::AlbumTarget::Existing(_)) => {
                                    "将加入指定的现有相册".to_owned()
                                }
                                Ok(backup::AlbumTarget::Create(name)) => {
                                    format!("将新建相册：{name}")
                                }
                                Err(error) => error,
                            }),
                    ),
            )
            .child(self.source(cx, self.queue.files.is_empty()))
            .when(!self.queue.files.is_empty(), |view| {
                view.child(self.files(cx))
            })
            .when(
                self.queue.files.is_empty() && !self.queue_errors.is_empty(),
                |view| {
                    view.children(self.queue_errors.iter().map(|error| {
                        div()
                            .text_sm()
                            .text_color(cx.theme().danger)
                            .child(error.clone())
                    }))
                },
            )
    }
}
