use super::*;
use gpui_kit::component::{
    IconName, Side, Sizable,
    alert::Alert,
    input::Input,
    menu::{DropdownMenu, PopupMenuItem},
};

impl UploadApp {
    pub(super) fn settings_menu(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        use gpui_kit::component::popover::Popover;

        Popover::new("settings-menu")
            .anchor(gpui_kit::Anchor::TopRight)
            .trigger(
                Button::new("settings")
                    .ghost()
                    .small()
                    .icon(IconName::Settings)
                    .accessibility_label("设置")
                    .tooltip("设置"),
            )
            .child(
                div()
                    .w(px(220.0))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(div().text_sm().child("上传并发数"))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        Button::new("threads-minus")
                                            .ghost()
                                            .small()
                                            .compact()
                                            .icon(IconName::Minus)
                                            .accessibility_label("减少并发数")
                                            .tooltip("减少并发数")
                                            .disabled(self.upload_threads <= 1)
                                            .on_click(cx.listener(|app, _, _, cx| {
                                                let threads = app.upload_threads.saturating_sub(1);
                                                app.set_threads(threads, cx);
                                            })),
                                    )
                                    .child(
                                        div()
                                            .w(px(24.0))
                                            .text_center()
                                            .text_sm()
                                            .child(self.upload_threads.to_string()),
                                    )
                                    .child(
                                        Button::new("threads-plus")
                                            .ghost()
                                            .small()
                                            .compact()
                                            .icon(IconName::Plus)
                                            .accessibility_label("增加并发数")
                                            .tooltip("增加并发数")
                                            .disabled(self.upload_threads >= 4)
                                            .on_click(cx.listener(|app, _, _, cx| {
                                                let threads = app.upload_threads + 1;
                                                app.set_threads(threads, cx);
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(theme(self.dark, MUTED)))
                            .child("范围 1–4，修改后对下一次上传生效"),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn account_menu(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let accounts = self.accounts.emails.clone();
        let active = self.accounts.active.clone();
        let busy = self.busy();
        let view = cx.entity();

        Button::new("account")
            .ghost()
            .small()
            .label(active.clone().unwrap_or_else(|| "选择账号".into()))
            .dropdown_caret(true)
            .max_w(px(260.0))
            .tooltip(active.clone().unwrap_or_else(|| "选择账号".into()))
            .disabled(busy)
            .dropdown_menu_with_anchor(gpui_kit::Anchor::TopLeft, move |menu, window, cx| {
                let menu_entity = cx.entity();
                let mut menu = menu
                    .min_w(px(250.0))
                    .max_h(px(340.0))
                    .scrollable(true)
                    .check_side(Side::Left);

                if accounts.is_empty() {
                    menu = menu.label("尚未添加账号");
                }
                for email in &accounts {
                    let selected = active.as_deref() == Some(email.as_str());
                    let email_for_select = email.clone();
                    let email_for_row = email.clone();
                    let email_for_remove = email.clone();
                    let row_view = view.clone();
                    let dismiss_menu = menu_entity.downgrade();

                    menu = menu.item(
                        PopupMenuItem::element(move |window, _| {
                            let email_for_remove = email_for_remove.clone();
                            let row_view = row_view.clone();
                            let dismiss_menu = dismiss_menu.clone();

                            div()
                                .w_full()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .child(email_for_row.clone()),
                                )
                                .child(
                                    Button::new(format!("remove-{email_for_remove}"))
                                        .ghost()
                                        .small()
                                        .compact()
                                        .icon(IconName::Delete)
                                        .accessibility_label(format!("移除 {email_for_remove}"))
                                        .tooltip("移除账号")
                                        .disabled(busy)
                                        .on_click(window.listener_for(
                                            &row_view,
                                            move |app, _, window, cx| {
                                                cx.stop_propagation();
                                                let _ = dismiss_menu.update(cx, |_, cx| {
                                                    cx.emit(gpui_kit::DismissEvent)
                                                });
                                                app.confirm_remove_account(
                                                    email_for_remove.clone(),
                                                    window,
                                                    cx,
                                                );
                                            },
                                        )),
                                )
                        })
                        .checked(selected)
                        .disabled(busy)
                        .on_click(window.listener_for(
                            &view,
                            move |app, _, _, cx| {
                                app.switch_account(email_for_select.clone(), cx);
                            },
                        )),
                    );
                }

                menu.separator().item(
                    PopupMenuItem::new("添加账号")
                        .icon(IconName::Plus)
                        .disabled(busy)
                        .on_click(window.listener_for(&view, |app, _, window, cx| {
                            app.open_auth(window, cx);
                        })),
                )
            })
            .into_any_element()
    }

    pub(super) fn connect(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        card(self.dark)
            .p_8()
            .gap_5()
            .child(
                div()
                    .size(px(56.0))
                    .rounded_xl()
                    .bg(rgb(theme(self.dark, ACCENT_TINT)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(illustration(
                        Illustration::Cloud,
                        28.0,
                        theme(self.dark, ACCENT),
                    )),
            )
            .child(
                div()
                    .text_xs()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .text_color(rgb(theme(self.dark, MUTED)))
                    .child("TINY PHOTOS  /  GOOGLE PHOTOS"),
            )
            .child(
                div()
                    .text_2xl()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(if self.loading_account {
                        "正在检查已保存的账号"
                    } else {
                        "照片与视频，轻松备份"
                    }),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(theme(self.dark, MUTED)))
                    .child("连接账号后，选择照片或视频，以原始质量备份到 Google Photos。"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(theme(self.dark, MUTED)))
                    .child("使用 Google Photos 非公开接口，服务变化可能影响备份；请核验云端文件。"),
            )
            .when(!self.loading_account, |view| {
                view.child(
                    div().pt_2().child(
                        button("connect", "连接 Google 账号", true, true)
                            .on_click(cx.listener(Self::connect_click)),
                    ),
                )
            })
    }
    pub(super) fn authorize(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let submit_label = if self.authenticating {
            "正在验证…"
        } else {
            "完成授权"
        };
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_5()
            .child(text_button("auth-back", "← 返回").disabled(self.authenticating).on_click(cx.listener(Self::back_from_auth)))
            .child(div().text_2xl().font_weight(gpui_kit::FontWeight::SEMIBOLD).child("连接 Google 账号"))
            .child(div().text_sm().text_color(rgb(theme(self.dark, MUTED))).child("完成一次授权，即可开始备份。"))
            .child(
                card(self.dark)
                    .gap_5()
                    .child(step_heading(self.dark, "1", "登录 Google", "在浏览器中打开授权页面并登录账号"))
                    .child(button("open-google", "打开授权页面 ↗", false, !self.authenticating)
                        .on_click(cx.listener(Self::open_google)))
                    .child(step_heading(self.dark, "2", "复制并粘贴授权令牌", "从浏览器 Cookie 中复制 oauth_token 的值"))
                    .child(Input::new(&self.token_input).disabled(self.authenticating))
                    .child(div().text_xs().text_color(rgb(theme(self.dark, MUTED))).child("令牌仅用于本次授权，不会保存。"))
                    .when_some(self.auth_error.as_ref(), |view, error| {
                        let guidance = match error.kind {
                            ProblemKind::Network => "检查网络后，可使用当前令牌重试。",
                            ProblemKind::Authorization => "请重新登录，复制新的 oauth_token 值后重试。",
                            ProblemKind::Other => "请检查令牌和系统凭据服务后重试。",
                        };
                        view.child(Alert::error("auth-error", format!("{}\n{}", error.message, guidance)).title("授权未完成"))
                    })
                    .child(button("paste-connect", submit_label, true, !self.authenticating && !self.auth_token.is_empty())
                        .on_click(cx.listener(Self::complete_auth)))
                    .child(text_button("auth-help", "如何获取 oauth_token？")
                        .on_click(cx.listener(Self::toggle_auth_help)))
                    .when(self.show_auth_help, |view| {
                        view.child(div().text_sm().text_color(rgb(theme(self.dark, MUTED)))
                            .child("登录后，在浏览器打开开发者工具，进入应用程序或存储，展开 Cookie 并选择 accounts.google.com。找到 oauth_token，只复制该项的值，不要复制名称或其他 Cookie。"))
                    }),
            )
    }
    pub(super) fn problem(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let issue = self.problem.as_ref().expect("账号异常状态缺少错误信息");
        let (icon, title, action) = match issue.kind {
            ProblemKind::Network => (Illustration::Network, "网络连接异常", "重试"),
            ProblemKind::Authorization => (Illustration::Lock, "授权不可用", "重新授权"),
            ProblemKind::Other => (Illustration::Network, "无法连接账号", "重试"),
        };
        card(self.dark)
            .p_8()
            .gap_4()
            .child(illustration(icon, 28.0, DANGER))
            .child(
                div()
                    .text_2xl()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(theme(self.dark, MUTED)))
                    .child(issue.message.clone()),
            )
            .when(matches!(issue.kind, ProblemKind::Authorization), |view| {
                view.child(
                    div().pt_2().child(
                        button("issue-reauthorize", action, true, true)
                            .on_click(cx.listener(Self::reauthorize)),
                    ),
                )
            })
            .when(!matches!(issue.kind, ProblemKind::Authorization), |view| {
                view.child(
                    div()
                        .flex()
                        .gap_2()
                        .pt_2()
                        .child(
                            button("issue-retry", action, true, true)
                                .on_click(cx.listener(Self::retry_problem)),
                        )
                        .child(
                            text_button("issue-change-account", "重新授权")
                                .on_click(cx.listener(Self::reauthorize)),
                        ),
                )
            })
    }
}
