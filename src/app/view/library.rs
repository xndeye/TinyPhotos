use super::*;

impl UploadApp {
    fn source(&self, cx: &mut Context<Self>, empty: bool) -> gpui_kit::Stateful<gpui_kit::Div> {
        let dark = self.dark;
        let zone = div()
            .id(if empty { "source-empty" } else { "source-more" })
            .w_full()
            .rounded_xl()
            .bg(rgb(theme(dark, SURFACE)))
            .border_1()
            .border_color(rgb(theme(dark, HAIRLINE)))
            .drag_over::<ExternalPaths>(move |style, _, _, _| {
                style
                    .bg(rgb(theme(dark, ACCENT_TINT)))
                    .border_color(rgb(theme(dark, ACCENT)))
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
                        .bg(rgb(theme(dark, ACCENT_TINT)))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(illustration(
                            Illustration::Upload,
                            24.0,
                            theme(dark, ACCENT),
                        )),
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
                        .text_color(rgb(theme(dark, MUTED)))
                        .child("拖入文件或文件夹，也可以从设备中选择。"),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .pt_2()
                        .child(
                            button("files-empty", "选择文件", true, !self.busy())
                                .on_click(cx.listener(Self::choose_files)),
                        )
                        .child(
                            button("folders-empty", "选择文件夹", false, !self.busy())
                                .on_click(cx.listener(Self::choose_folders)),
                        ),
                )
        } else {
            zone.p_4()
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
                                .text_color(rgb(theme(dark, MUTED)))
                                .child("也可拖入文件或文件夹"),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            button("files-more", "添加文件", false, !self.busy())
                                .on_click(cx.listener(Self::choose_files)),
                        )
                        .child(
                            button("folders-more", "添加文件夹", false, !self.busy())
                                .on_click(cx.listener(Self::choose_folders)),
                        ),
                )
        }
    }
    pub(super) fn empty(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_6()
            .mt_6()
            .child(
                div()
                    .text_2xl()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child("选择要备份的内容"),
            )
            .child(self.source(cx, true))
            .when(!self.errors.is_empty(), |view| {
                view.children(self.errors.iter().map(|error| {
                    div()
                        .text_sm()
                        .text_color(rgb(theme(self.dark, DANGER)))
                        .child(error.clone())
                }))
            })
    }

    fn files(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let count = self.queue.files.len();
        let pages = count.div_ceil(PAGE_SIZE);
        div()
            .w_full()
            .rounded_lg()
            .border_1()
            .border_color(rgb(theme(self.dark, HAIRLINE)))
            .bg(rgb(theme(self.dark, SURFACE)))
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
                                    .text_color(rgb(theme(self.dark, MUTED)))
                                    .child(format!(
                                        "{count} 个文件 · {}",
                                        format_size(self.queue.total_bytes)
                                    )),
                            ),
                    )
                    .child(
                        text_button("clear-queue", "清空").on_click(cx.listener(Self::clear_queue)),
                    ),
            )
            .child(div().h(px(1.0)).bg(rgb(theme(self.dark, HAIRLINE))))
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
                                    .bg(rgb(theme(self.dark, ACCENT_TINT)))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_xs()
                                    .text_color(rgb(theme(self.dark, MUTED)))
                                    .child(file_icon(&item.path)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(div().text_sm().child(item.name.clone()))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(theme(self.dark, MUTED)))
                                            .child(item.path.to_string_lossy().into_owned()),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(theme(self.dark, MUTED)))
                                    .child(format_size(item.size)),
                            )
                            .child(
                                text_button(format!("remove-{index}"), "×")
                                    .accessibility_label(format!("从队列移除 {}", item.name))
                                    .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                                        app.remove_file(index, cx);
                                    })),
                            )
                    }),
            )
            .when(pages > 1, |view| {
                view.child(div().h(px(1.0)).bg(rgb(theme(self.dark, HAIRLINE))))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(theme(self.dark, MUTED)))
                                    .child(format!("{}/{} 页", self.page + 1, pages)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .when(self.page > 0, |row| {
                                        row.child(
                                            text_button("prev", "上一页")
                                                .on_click(cx.listener(Self::previous_page)),
                                        )
                                    })
                                    .when(self.page + 1 < pages, |row| {
                                        row.child(
                                            text_button("next", "下一页")
                                                .on_click(cx.listener(Self::next_page)),
                                        )
                                    }),
                            ),
                    )
            })
            .when(!self.errors.is_empty(), |view| {
                view.child(div().h(px(1.0)).bg(rgb(theme(self.dark, HAIRLINE))))
                    .children(self.errors.iter().map(|error| {
                        div()
                            .text_xs()
                            .text_color(rgb(theme(self.dark, DANGER)))
                            .child(error.clone())
                    }))
            })
    }

    pub(super) fn review(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
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
            .child(self.source(cx, false))
            .child(self.files(cx))
            .child(
                card(self.dark)
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
                            .text_color(rgb(theme(self.dark, MUTED)))
                            .child("留空仅备份文件；可输入普通相册链接、ID 或新相册名称"),
                    )
                    .when_some(self.album_error.as_ref(), |view, error| {
                        view.child(
                            div()
                                .text_sm()
                                .text_color(rgb(theme(self.dark, DANGER)))
                                .child(error.clone()),
                        )
                    }),
            )
    }
}
