//! Credential lifecycle and account switching. Secrets stay in OS storage.
use super::*;

fn decode_credentials(email: &str, secret: &[u8]) -> Result<Credentials, backup::Error> {
    let raw = std::str::from_utf8(secret).map_err(|error| {
        backup::Error::new(
            backup::ErrorKind::Authorization,
            format!("保存的账号凭据格式无效：{error}"),
        )
    })?;
    let credential = Credentials::parse(raw)?;
    if accounts::normalized_email(&credential.email) != email {
        return Err(backup::Error::new(
            backup::ErrorKind::Authorization,
            "系统凭据与账号索引不匹配",
        ));
    }
    Ok(credential)
}

#[derive(Default, PartialEq, Eq)]
pub(super) enum AccountOperation {
    #[default]
    Idle,
    Loading,
    Authorizing,
}

#[derive(Default)]
pub(super) struct AccountState {
    pub(super) credentials: Option<Credentials>,
    pub(super) index: accounts::AccountIndex,
    pub(super) operation: AccountOperation,
    pub(super) auth_open: bool,
    pub(super) show_auth_help: bool,
    pub(super) auth_error: Option<backup::Error>,
    pub(super) problem: Option<backup::Error>,
}

impl UploadApp {
    fn reset_account_context(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.backup = BackupTask::default();
        self.queue_errors.clear();
        self.thumbnails.clear();
        self.album_input
            .update(cx, |input, cx| input.set_value("", window, cx));
    }

    pub(super) fn verify_saved_account(&mut self, cx: &mut Context<Self>) {
        self.account.operation = AccountOperation::Loading;
        cx.spawn(async move |this, cx| {
            let initialized: Result<
                (
                    accounts::AccountIndex,
                    Result<Option<Credentials>, backup::Error>,
                ),
                backup::Error,
            > = async {
                let index = cx
                    .background_spawn(async { accounts::AccountIndex::load() })
                    .await?;
                let verified: Result<Option<Credentials>, backup::Error> = async {
                    let Some(email) = index.active.as_ref() else {
                        return Ok(None);
                    };
                    let key = accounts::credential_key(email);
                    let (_, secret) = cx
                        .update(|app| app.read_credentials(&key))
                        .await
                        .map_err(|error| error.to_string())?
                        .ok_or_else(|| {
                            backup::Error::new(
                                backup::ErrorKind::Authorization,
                                "当前账号的系统凭据不存在，请重新授权",
                            )
                        })?;
                    let credential = decode_credentials(email, &secret)?;
                    cx.background_spawn(async move { backup::verify_account(credential) })
                        .await
                        .map(Some)
                }
                .await;
                Ok((index, verified))
            }
            .await;
            let _ = this.update(cx, |app, cx| {
                app.account.operation = AccountOperation::Idle;
                match initialized {
                    Ok((index, Ok(credential))) => {
                        app.account.index = index;
                        app.account.credentials = credential;
                        app.account.problem = None;
                    }
                    Ok((index, Err(error))) => {
                        app.account.index = index; // Still allow choosing a different saved account.
                        app.account.credentials = None;
                        app.account.problem = Some(error);
                    }
                    Err(error) => {
                        app.account.credentials = None;
                        app.account.problem = Some(error);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn switch_account(
        &mut self,
        email: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy() {
            return;
        }
        if self.account.index.active.as_deref() == Some(&email)
            && self.account.credentials.is_some()
        {
            return;
        }
        self.account.operation = AccountOperation::Loading;
        self.notice = "正在切换账号…".into();
        let mut next = self.account.index.clone();
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result: Result<(accounts::AccountIndex, Credentials), backup::Error> = async {
                let key = accounts::credential_key(&email);
                let (_, secret) = cx
                    .update(|_, app| app.read_credentials(&key))
                    .map_err(|error| error.to_string())?
                    .await
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| {
                        backup::Error::new(
                            backup::ErrorKind::Authorization,
                            "该账号的系统凭据不存在，请重新添加",
                        )
                    })?;
                let credential = decode_credentials(&email, &secret)?;
                let credential = cx
                    .background_spawn(async move { backup::verify_account(credential) })
                    .await?;
                next.active = Some(email);
                let saved = next.clone();
                cx.background_spawn(async move { saved.save() }).await?;
                Ok((next, credential))
            }
            .await;
            let _ = this.update_in(cx, |app, window, cx| {
                app.account.operation = AccountOperation::Idle;
                match result {
                    Ok((index, credential)) => {
                        app.reset_account_context(window, cx);
                        app.account.index = index;
                        app.account.credentials = Some(credential);
                        app.account.problem = None;
                        app.notice = "已切换账号，本地待备份文件未改变".into();
                    }
                    Err(error) => app.notice = format!("切换账号失败：{error}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn confirm_remove_account(
        &mut self,
        email: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy() {
            return;
        }
        let view = cx.entity();
        use gpui_kit::component::{WindowExt as _, button::ButtonVariant};
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let view = view.clone();
            let account = email.clone();
            dialog.title("移除账号")
                .description(format!(
                    "将从 TinyPhotos 移除 {email} 的本地授权，不会删除本地照片或 Google Photos 中的文件。"
                ))
                .confirm()
                .ok_text("移除账号")
                .ok_variant(ButtonVariant::Danger)
                .cancel_text("取消")
                .on_ok(move |_, window, cx| {
                    view.update(cx, |app, cx| app.remove_account(account.clone(), window, cx));
                    true
                })
        });
    }

    fn remove_account(&mut self, email: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        self.account.operation = AccountOperation::Loading;
        let original = self.account.index.clone();
        let was_active = original.active.as_deref() == Some(email.as_str());
        let mut next = original.clone();
        next.remove(&email);
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result: Result<accounts::AccountIndex, backup::Error> = async {
                let saved = next.clone();
                cx.background_spawn(async move { saved.save() }).await?;
                let key = accounts::credential_key(&email);
                if let Err(error) = cx
                    .update(|_, app| app.delete_credentials(&key))
                    .map_err(|error| error.to_string())?
                    .await
                {
                    cx.background_spawn(async move { original.save() })
                        .await
                        .map_err(|restore| {
                            format!("无法删除账号凭据：{error}；恢复账号索引失败：{restore}")
                        })?;
                    return Err(format!("无法删除账号凭据：{error}").into());
                }
                Ok(next)
            }
            .await;
            let _ = this.update_in(cx, |app, window, cx| {
                app.account.operation = AccountOperation::Idle;
                match result {
                    Ok(index) => {
                        app.account.index = index;
                        if was_active {
                            app.account.credentials = None;
                            app.account.problem = None;
                            app.account.auth_open = false;
                            app.reset_account_context(window, cx);
                            if app.account.index.active.is_some() {
                                app.verify_saved_account(cx);
                            }
                        }
                        app.notice = "账号已移除".into();
                    }
                    Err(error) => app.notice = format!("移除账号失败：{error}"),
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn import_account_hotkey(
        &mut self,
        _: &ImportAccount,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_auth(window, cx);
    }

    pub(super) fn open_auth(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        self.account.auth_open = true;
        self.account.show_auth_help = false;
        self.account.auth_error = None;
        if self.backup.state != TaskState::Done {
            self.account.problem = None;
        }
        self.token_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.notice.clear();
        cx.notify();
        let focus = self.token_input.focus_handle(cx);
        window.defer(cx, move |window, cx| focus.focus(window, cx));
    }

    pub(super) fn back_from_auth(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.account.operation == AccountOperation::Authorizing {
            return;
        }
        self.account.auth_open = false;
        if self.backup.state != TaskState::Done {
            self.account.problem = None;
        }
        self.account.auth_error = None;
        self.token_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.notice.clear();
        cx.notify();
        self.focus.focus(window, cx);
    }

    pub(super) fn open_google(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        cx.open_url("https://accounts.google.com/EmbeddedSetup");
    }

    pub(super) fn toggle_auth_help(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.account.show_auth_help = !self.account.show_auth_help;
        cx.notify();
    }

    pub(super) fn complete_auth(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.begin_auth(window, cx);
    }

    pub(super) fn begin_auth(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        let token = self.token_input.read(cx).value().to_string();
        let index = self.account.index.clone();
        self.account.auth_error = None;
        self.account.operation = AccountOperation::Authorizing;
        self.notice = "正在验证 Google 授权…".into();
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    backup::exchange_embedded(&token).and_then(backup::verify_account)
                })
                .await;
            let result: Result<(Credentials, accounts::AccountIndex), backup::Error> = async {
                let credential = result?;
                let key = accounts::credential_key(&credential.email);
                let serialized = credential.serialized();
                cx.update(|_, app| {
                    app.write_credentials(&key, &credential.email, serialized.as_bytes())
                })
                .map_err(|error| error.to_string())?
                .await
                .map_err(|e| format!("账号已验证，但系统凭据存储失败：{e}"))?;
                let mut next = index;
                next.add(&credential.email);
                let saved = next.clone();
                cx.background_spawn(async move { saved.save() })
                    .await
                    .map_err(|e| format!("无法保存账号索引：{e}"))?;
                Ok((credential, next))
            }
            .await;
            let _ = this.update_in(cx, |app, window, cx| {
                app.account.operation = AccountOperation::Idle;
                match result {
                    Ok((credential, index)) => {
                        let changed = app.account.credentials.as_ref().is_none_or(|previous| {
                            accounts::normalized_email(&previous.email)
                                != accounts::normalized_email(&credential.email)
                        });
                        if changed {
                            app.reset_account_context(window, cx);
                        }
                        app.account.index = index;
                        app.account.credentials = Some(credential);
                        app.account.auth_open = false;
                        app.token_input
                            .update(cx, |input, cx| input.set_value("", window, cx));
                        app.account.problem = None;
                        app.notice = "账号已连接".into();
                    }
                    Err(error) => {
                        app.account.auth_error = Some(error);
                        app.notice.clear();
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn retry_problem(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        self.account.problem = None;
        self.verify_saved_account(cx);
        cx.notify();
    }

    pub(super) fn reauthorize(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_auth(window, cx);
    }
}
