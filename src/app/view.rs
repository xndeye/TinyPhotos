//! One primary action per task state. Avoid duplicate headings, status badges and instructions.
use super::*;
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, TitleBar,
    button::{Button, ButtonVariants},
    progress::Progress,
    tooltip::Tooltip,
};
use gpui_kit::{Render, div, img, prelude::*, px};

mod account;
mod footer;
mod library;
mod progress;
mod results;

#[derive(Clone, Copy)]
enum Illustration {
    Upload,
    Network,
    Lock,
}

fn illustration(kind: Illustration, size: f32, color: gpui_kit::Hsla) -> impl IntoElement {
    let path = match kind {
        Illustration::Upload => "icons/arrow-up.svg",
        Illustration::Network => "icons/circle-alert.svg",
        Illustration::Lock => "icons/triangle-alert.svg",
    };
    Icon::default().path(path).size(px(size)).text_color(color)
}

fn card(cx: &App) -> gpui_kit::Div {
    div()
        .w_full()
        .rounded_lg()
        .bg(cx.theme().background)
        .border_1()
        .border_color(cx.theme().border)
        .p_7()
        .flex()
        .flex_col()
        .gap_5()
}

fn step_heading(
    cx: &App,
    number: &'static str,
    title: &'static str,
    description: &'static str,
) -> gpui_kit::Div {
    div()
        .flex()
        .items_start()
        .gap_3()
        .child(
            div()
                .size(px(28.0))
                .rounded_full()
                .bg(cx.theme().primary)
                .text_color(cx.theme().primary_foreground)
                .flex()
                .items_center()
                .justify_center()
                .child(number),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .child(title),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(description),
                ),
        )
}

fn file_text(id: String, text: String) -> gpui_kit::Stateful<gpui_kit::Div> {
    let tooltip = text.clone();
    div()
        .id(id)
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .child(text)
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
}

fn progress_bar(value: f32) -> Progress {
    Progress::new("upload-progress")
        .accessibility_label("文件处理进度")
        .value(value * 100.0)
}

fn thumbnail(cx: &App, path: &std::path::Path, preview: Option<&Thumbnail>) -> gpui_kit::Div {
    let container = div()
        .size(px(48.0))
        .rounded_md()
        .overflow_hidden()
        .bg(cx.theme().muted)
        .flex()
        .items_center()
        .justify_center();
    if let Some(Thumbnail::Ready(image)) = preview {
        container.child(
            img(image.clone())
                .size_full()
                .object_fit(gpui_kit::ObjectFit::Cover),
        )
    } else {
        container.child(
            div()
                .text_color(cx.theme().muted_foreground)
                .child(file_icon(path)),
        )
    }
}

fn file_icon(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some(
            "mp4" | "mov" | "m4v" | "avi" | "mkv" | "webm" | "3gp" | "3g2" | "asf" | "divx" | "m2t"
            | "m2ts" | "mmv" | "mod" | "mpg" | "mpeg" | "mts" | "tod" | "ts" | "wmv",
        ) => "▶",
        Some("dng" | "cr2" | "cr3" | "nef" | "arw" | "orf" | "raf" | "rw2" | "pef" | "sr2") => {
            "RAW"
        }
        _ => "▧",
    }
}

fn result_metric(
    cx: &App,
    label: &'static str,
    count: usize,
    color: gpui_kit::Hsla,
) -> gpui_kit::Div {
    div()
        .flex_1()
        .min_w(px(120.0))
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border)
        .p_4()
        .flex()
        .flex_col()
        .items_start()
        .gap_2()
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(
            div()
                .text_2xl()
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .text_color(color)
                .child(count.to_string()),
        )
}
fn format_size(bytes: u64) -> String {
    let value = bytes as f64;
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GiB", value / 1024f64.powi(3))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", value / 1024f64.powi(2))
    } else if bytes >= 1024 {
        format!("{:.1} KiB", value / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

impl Render for UploadApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let screen = self.screen();
        let body = match screen {
            Screen::Connect => self.authorize(cx).into_any_element(),
            Screen::Authorize => self.authorize(cx).into_any_element(),
            Screen::Problem(error) => self.problem(error, cx).into_any_element(),
            Screen::Empty => self.review(cx).into_any_element(),
            Screen::Review => self.review(cx).into_any_element(),
            Screen::Uploading => self.uploading(cx).into_any_element(),
            Screen::Results => self.results(cx).into_any_element(),
        };
        div()
            .relative()
            .size_full()
            .key_context("TinyPhotos")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::open_files))
            .on_action(cx.listener(Self::open_folders))
            .on_action(cx.listener(Self::begin_backup))
            .on_action(cx.listener(Self::import_account_hotkey))
            .on_action(cx.listener(Self::pause_hotkey))
            .on_action(cx.listener(Self::stop_hotkey))
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                TitleBar::new()
                    .child(
                        div().flex().items_center().gap_3().child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .text_sm()
                                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                .child(img(self.app_icon.clone()).size(px(20.0)))
                                .child("TinyPhotos"),
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .items_center()
                            .justify_end()
                            .gap_2()
                            .pr_2()
                            .when(!self.account.auth_open, |bar| {
                                bar.child(
                                    div()
                                        .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation()
                                        })
                                        .child(self.account_menu(window, cx)),
                                )
                            })
                            .child(
                                div()
                                    .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation();
                                    })
                                    .child(self.settings_menu(cx)),
                            ),
                    ),
            )
            .child(
                div()
                    .id("content")
                    .flex_1()
                    .overflow_y_scroll()
                    .p_6()
                    .flex()
                    .flex_col()
                    .items_center()
                    .when(
                        matches!(screen, Screen::Uploading | Screen::Problem(_)),
                        |view| view.justify_center(),
                    )
                    .child(
                        div()
                            .w_full()
                            .when(
                                matches!(screen, Screen::Connect | Screen::Authorize),
                                |view| view.max_w(px(680.0)),
                            )
                            .child(body),
                    ),
            )
            .when(
                !self.notice.is_empty()
                    || self.undo.is_some()
                    || matches!(screen, Screen::Review | Screen::Uploading | Screen::Results),
                |view| view.child(self.footer(cx)),
            )
    }
}
