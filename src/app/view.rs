//! One primary action per task state. Avoid duplicate headings, status badges and instructions.
use super::*;
use gpui_kit::component::{
    Disableable, Icon, Theme, TitleBar,
    button::{Button, ButtonVariants},
    progress::Progress,
};
use gpui_kit::{Render, div, img, prelude::*, px, rgb};

mod account;
mod library;
mod progress;

#[derive(Clone, Copy)]
enum Illustration {
    Cloud,
    Upload,
    Network,
    Lock,
}

fn illustration(kind: Illustration, size: f32, color: u32) -> impl IntoElement {
    let path = match kind {
        Illustration::Cloud => "icons/arrow-up.svg",
        Illustration::Upload => "icons/arrow-up.svg",
        Illustration::Network => "icons/circle-alert.svg",
        Illustration::Lock => "icons/triangle-alert.svg",
    };
    Icon::default()
        .path(path)
        .size(px(size))
        .text_color(rgb(color))
}
fn theme(dark: bool, color: u32) -> u32 {
    if !dark {
        return color;
    }
    match color {
        BACKGROUND => 0x171A18,
        SURFACE => 0x222623,
        INK => 0xF2F4EF,
        MUTED => 0xA6AEA7,
        HAIRLINE => 0x383E39,
        ACCENT_TINT => 0x2B322D,
        ACCENT => 0xD6E6D9,
        SUCCESS => 0x80CDA4,
        DANGER => 0xF09B94,

        _ => color,
    }
}

fn card(dark: bool) -> gpui_kit::Div {
    div()
        .w_full()
        .rounded_lg()
        .bg(rgb(theme(dark, SURFACE)))
        .border_1()
        .border_color(rgb(theme(dark, HAIRLINE)))
        .p_7()
        .flex()
        .flex_col()
        .gap_5()
}

fn step_heading(
    dark: bool,
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
                .bg(rgb(theme(dark, ACCENT)))
                .text_color(rgb(if dark { INK } else { 0xFFFFFF }))
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
                        .text_color(rgb(theme(dark, MUTED)))
                        .child(description),
                ),
        )
}

fn button(
    id: impl Into<gpui_kit::ElementId>,
    label: &'static str,
    primary: bool,
    enabled: bool,
) -> Button {
    let button = Button::new(id).label(label).disabled(!enabled);
    if primary { button.primary() } else { button }
}

fn text_button(id: impl Into<gpui_kit::ElementId>, label: &'static str) -> Button {
    Button::new(id).label(label).ghost()
}

fn progress_bar(value: f32) -> Progress {
    Progress::new("upload-progress")
        .accessibility_label("文件处理进度")
        .value(value.clamp(0.0, 1.0) * 100.0)
}

fn thumbnail(
    dark: bool,
    path: Option<&std::path::Path>,
    preview: Option<&Thumbnail>,
) -> gpui_kit::Div {
    let container = div()
        .size(px(48.0))
        .rounded_md()
        .overflow_hidden()
        .bg(rgb(theme(dark, ACCENT_TINT)))
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
                .text_color(rgb(theme(dark, MUTED)))
                .child(path.map_or("▧", file_icon)),
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

fn result_metric(dark: bool, label: &'static str, count: usize, color: u32) -> gpui_kit::Div {
    div()
        .flex_1()
        .min_w(px(120.0))
        .rounded_lg()
        .border_1()
        .border_color(rgb(theme(dark, HAIRLINE)))
        .p_4()
        .flex()
        .flex_col()
        .items_start()
        .gap_2()
        .child(
            div()
                .text_xs()
                .text_color(rgb(theme(dark, MUTED)))
                .child(label),
        )
        .child(
            div()
                .text_2xl()
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .text_color(rgb(theme(dark, color)))
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
        if self.clear_auth_input {
            self.clear_auth_input = false;
            self.token_input
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
        if self.clear_album_input {
            self.clear_album_input = false;
            self.album_input
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.dark = matches!(
            window.appearance(),
            gpui_kit::WindowAppearance::Dark | gpui_kit::WindowAppearance::VibrantDark
        );
        if self.theme_subscription.is_none() {
            self.theme_subscription = Some(window.observe_window_appearance(|window, cx| {
                Theme::sync_system_appearance(Some(window), cx)
            }));
        }
        let screen = self.screen();
        let body = match screen {
            Screen::Connect => self.connect(cx).into_any_element(),
            Screen::Authorize => self.authorize(cx).into_any_element(),
            Screen::Problem => self.problem(cx).into_any_element(),
            Screen::Details => self.details(cx).into_any_element(),
            Screen::Empty => self.empty(cx).into_any_element(),
            Screen::Review => self.review(cx).into_any_element(),
            Screen::Uploading => self.uploading().into_any_element(),
            Screen::Results => self.results().into_any_element(),
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
            .flex()
            .flex_col()
            .bg(rgb(theme(self.dark, BACKGROUND)))
            .text_color(rgb(theme(self.dark, INK)))
            .child(
                TitleBar::new()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .text_sm()
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .child(img(self.app_icon.clone()).size(px(20.0)))
                                    .child("TinyPhotos"),
                            )
                            .when(!self.auth_open, |bar| {
                                bar.child(
                                    div()
                                        .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation()
                                        })
                                        .child(self.account_menu(cx)),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .pr_2()
                            .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            .child(self.settings_menu(cx)),
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
                        matches!(
                            screen,
                            Screen::Connect
                                | Screen::Empty
                                | Screen::Uploading
                                | Screen::Results
                                | Screen::Problem
                        ),
                        |view| view.justify_center(),
                    )
                    .child(div().w_full().max_w(px(680.0)).child(body)),
            )
            .when(
                !self.notice.is_empty()
                    || self.undo.is_some()
                    || matches!(screen, Screen::Review | Screen::Uploading | Screen::Results),
                |view| view.child(self.footer(cx)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn human_file_sizes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1024), "1.0 KiB");
        assert_eq!(format_size(1024 * 1024), "1.0 MiB");
    }
}
