use super::*;
use gpui_kit::base::actions::{Cancel, Confirm, SelectDown, SelectUp};
use gpui_kit::component::{
    IconName, Side, Sizable,
    alert::Alert,
    input::Input,
    menu::{DropdownMenu, PopupMenuItem},
};
use std::rc::Rc;

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
                                                let threads = app.upload_threads - 1;
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
                            .text_color(cx.theme().muted_foreground)
                            .child("范围 1–4，修改后对下一次上传生效"),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn account_menu(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let accounts = self.account.index.emails.clone();
        let active = self.account.index.active.clone();
        let busy = self.busy();
        let view = cx.entity();
        let focus = window
            .use_keyed_state("account-menu-focus", cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();

        Button::new("account")
            .track_focus(&focus)
            .ghost()
            .small()
            .label(active.clone().unwrap_or_else(|| "选择账号".into()))
            .dropdown_caret(true)
            .max_w(px(260.0))
            .tooltip(active.clone().unwrap_or_else(|| "选择账号".into()))
            .disabled(busy)
            .dropdown_menu_with_anchor(gpui_kit::Anchor::TopLeft, move |menu, window, cx| {
                let menu_focus = menu.focus_handle(cx);
                let mut menu = menu
                    .action_context(focus.clone())
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
                    let email = email.clone();
                    let view_for_remove = view.clone();
                    let focus_for_remove = focus.clone();
                    let menu_focus = menu_focus.clone();
                    menu = menu.item(
                        PopupMenuItem::element(move |window, cx| {
                            let account = email.clone();
                            let focus = focus_for_remove.clone();
                            let remove = Rc::new(window.listener_for(
                                &view_for_remove,
                                move |_, _: &ClickEvent, window, cx| {
                                    cx.stop_propagation();
                                    window.dispatch_action(Box::new(Cancel), cx);
                                    let account = account.clone();
                                    let focus = focus.clone();
                                    cx.defer_in(window, move |app, window, cx| {
                                        focus.focus(window, cx);
                                        app.confirm_remove_account(account, window, cx);
                                    });
                                },
                            ));
                            div()
                                .flex_1()
                                .min_w_0()
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
                                        .child(email.clone()),
                                )
                                .child(
                                    Button::new(format!("remove-account-{email}"))
                                        .ghost()
                                        .small()
                                        .compact()
                                        .icon(gpui_kit::assets::IconName::Trash)
                                        .accessibility_label(format!("移除账号 {email}"))
                                        .tooltip("移除账号")
                                        .disabled(busy)
                                        .when(!busy, |button| {
                                            button
                                                .hover(|style| style.text_color(cx.theme().danger))
                                        })
                                        .on_click({
                                            let remove = Rc::clone(&remove);
                                            move |event, window, cx| remove(event, window, cx)
                                        })
                                        .on_action(move |_: &Confirm, window, cx| {
                                            remove(&ClickEvent::default(), window, cx);
                                        })
                                        .on_action({
                                            let focus = menu_focus.clone();
                                            move |_: &SelectUp, window, cx| {
                                                focus.focus(window, cx);
                                                cx.propagate();
                                            }
                                        })
                                        .on_action({
                                            let focus = menu_focus.clone();
                                            move |_: &SelectDown, window, cx| {
                                                focus.focus(window, cx);
                                                cx.propagate();
                                            }
                                        }),
                                )
                        })
                        .checked(selected)
                        .disabled(busy)
                        .on_click(window.listener_for(
                            &view,
                            move |app, _, window, cx| {
                                app.switch_account(email_for_select.clone(), window, cx);
                            },
                        )),
                    );
                }
                menu.separator().item(
                    PopupMenuItem::new("添加账号…")
                        .icon(IconName::Plus)
                        .disabled(busy)
                        .on_click(window.listener_for(&view, |app, _, window, cx| {
                            app.open_auth(window, cx);
                        })),
                )
            })
            .into_any_element()
    }

    pub(super) fn authorize(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let submit_label = if self.account.operation == AccountOperation::Authorizing {
            "正在验证…"
        } else {
            "完成授权"
        };
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_5()
            .when(self.account.auth_open, |view| view.child(Button::new("auth-back").label("返回").ghost().disabled(self.account.operation == AccountOperation::Authorizing).on_click(cx.listener(Self::back_from_auth))))
            .child(div().text_2xl().font_weight(gpui_kit::FontWeight::SEMIBOLD).child("连接 Google 账号"))
            .child(div().text_sm().text_color(cx.theme().muted_foreground).child("完成一次授权，即可开始备份。"))
            .when(self.account.operation == AccountOperation::Loading, |view| view.child(div().text_sm().child("正在检查已保存的账号…")))
            .child(
                card(cx)
                    .gap_5()
                    .child(step_heading(cx, "1", "登录 Google", "在浏览器中打开授权页面并登录账号"))
                    .child(Button::new("open-google").label("打开授权页面 ↗").disabled(self.busy())
                        .on_click(cx.listener(Self::open_google)))
                    .child(step_heading(cx, "2", "复制并粘贴授权令牌", "从浏览器 Cookie 中复制 oauth_token 的值"))
                    .child(Input::new(&self.token_input).disabled(self.busy()))
                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child("令牌仅用于本次授权，不会保存。"))
                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child("使用 Google Photos 非公开接口，服务变化可能影响备份；请核验云端文件。"))
                    .when_some(self.account.auth_error.as_ref(), |view, error| {
                        let guidance = match error.kind {
                            backup::ErrorKind::Network => "检查网络后，可使用当前令牌重试。",
                            backup::ErrorKind::Authorization => "请重新登录，复制新的 oauth_token 值后重试。",
                            _ => "请检查令牌和系统凭据服务后重试。",
                        };
                        view.child(Alert::error("auth-error", format!("{}\n{}", error.message, guidance)).title("授权未完成"))
                    })
                    .child(Button::new("paste-connect").label(submit_label).primary().disabled(self.busy() || self.token_input.read(cx).value().is_empty())
                        .on_click(cx.listener(Self::complete_auth)))
                    .child(Button::new("auth-help").label("如何获取 oauth_token？").ghost()
                        .on_click(cx.listener(Self::toggle_auth_help)))
                    .when(self.account.show_auth_help, |view| {
                        view.child(div().text_sm().text_color(cx.theme().muted_foreground)
                            .child("登录后，在浏览器打开开发者工具，进入应用程序或存储，展开 Cookie 并选择 accounts.google.com。找到 oauth_token，只复制该项的值，不要复制名称或其他 Cookie。"))
                    }),
            )
    }
    pub(super) fn problem(&self, issue: &backup::Error, cx: &mut Context<Self>) -> gpui_kit::Div {
        let (icon, title, action) = match issue.kind {
            backup::ErrorKind::Network => (Illustration::Network, "网络连接异常", "重试"),
            backup::ErrorKind::Authorization => (Illustration::Lock, "授权不可用", "重新授权"),
            _ => (Illustration::Network, "无法连接账号", "重试"),
        };
        card(cx)
            .p_8()
            .gap_4()
            .child(illustration(icon, 28.0, cx.theme().danger))
            .child(
                div()
                    .text_2xl()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(issue.message.clone()),
            )
            .when(
                matches!(issue.kind, backup::ErrorKind::Authorization),
                |view| {
                    view.child(
                        div().pt_2().child(
                            Button::new("issue-reauthorize")
                                .label(action)
                                .primary()
                                .on_click(cx.listener(Self::reauthorize)),
                        ),
                    )
                },
            )
            .when(
                !matches!(issue.kind, backup::ErrorKind::Authorization),
                |view| {
                    view.child(
                        div()
                            .flex()
                            .gap_2()
                            .pt_2()
                            .child(
                                Button::new("issue-retry")
                                    .label(action)
                                    .primary()
                                    .on_click(cx.listener(Self::retry_problem)),
                            )
                            .child(
                                Button::new("issue-change-account")
                                    .label("重新授权")
                                    .ghost()
                                    .on_click(cx.listener(Self::reauthorize)),
                            ),
                    )
                },
            )
    }
}
