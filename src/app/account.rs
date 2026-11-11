//! Credential lifecycle and account switching. Secrets stay in OS storage.
use super::*;

impl UploadApp {
    pub(super) fn reset_account_context(&mut self) {
        self.task = TaskState::Idle;
        self.task_error = None;
        self.show_details = false;
        self.outcomes.clear();
        self.errors.clear();
        self.thumbnails.clear();
        self.album_text.clear();
        self.album_target = None;
        self.album_error = None;
        self.clear_album_input = true;
        self.reset_progress();
    }

    pub(super) fn verify_saved_account(&mut self, cx: &mut Context<Self>) {
        self.loading_account = true;
        cx.spawn(async move |this, cx| {
            let initialized: Result<
                (accounts::AccountIndex, Result<Option<Credentials>, String>),
                String,
            > = async {
                let index = cx
                    .background_spawn(async { accounts::AccountIndex::load() })
                    .await?;
                let verified: Result<Option<Credentials>, String> = async {
                    let Some(email) = index.active.as_ref() else {
                        return Ok(None);
                    };
                    let key = accounts::credential_key(email);
                    let (_, secret) = cx
                        .update(|app| app.read_credentials(&key))
                        .await
                        .map_err(|error| error.to_string())?
                        .ok_or("当前账号的系统凭据不存在，请重新授权")?;
                    let credential = String::from_utf8(secret)
                        .map_err(|_| "保存的账号凭据格式无效，请重新授权".to_owned())
                        .and_then(|raw| Credentials::parse(&raw))?;
                    if accounts::normalized_email(&credential.email) != *email {
                        return Err("系统凭据与账号索引不匹配".into());
                    }
                    cx.background_spawn(async move { backup::verify_account(credential) })
                        .await
                        .map(Some)
                }
                .await;
                Ok((index, verified))
            }
            .await;
            let _ = this.update(cx, |app, cx| {
                app.loading_account = false;
                match initialized {
                    Ok((index, Ok(credential))) => {
                        app.accounts = index;
                        app.credentials = credential;
                        app.problem = None;
                    }
                    Ok((index, Err(error))) => {
                        app.accounts = index; // Still allow choosing a different saved account.
                        app.credentials = None;
                        app.problem = Some(classify_problem(error));
                    }
                    Err(error) => {
                        app.credentials = None;
                        app.problem = Some(classify_problem(error));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn switch_account(&mut self, email: String, cx: &mut Context<Self>) {
        if self.busy() || !self.accounts.emails.contains(&email) {
            return;
        }
        if self.accounts.active.as_deref() == Some(&email) && self.credentials.is_some() {
            return;
        }
        self.loading_account = true;
        self.notice = "正在切换账号…".into();
        let mut next = self.accounts.clone();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result: Result<(accounts::AccountIndex, Credentials), String> = async {
                let key = accounts::credential_key(&email);
                let (_, secret) = cx
                    .update(|app| app.read_credentials(&key))
                    .await
                    .map_err(|error| error.to_string())?
                    .ok_or("该账号的系统凭据不存在，请重新添加")?;
                let credential = String::from_utf8(secret)
                    .map_err(|_| "保存的账号凭据格式无效".to_owned())
                    .and_then(|raw| Credentials::parse(&raw))?;
                if accounts::normalized_email(&credential.email) != email {
                    return Err("账号和保存的凭据不匹配".into());
                }
                let credential = cx
                    .background_spawn(async move { backup::verify_account(credential) })
                    .await?;
                next.active = Some(email);
                let saved = next.clone();
                cx.background_spawn(async move { saved.save() }).await?;
                Ok((next, credential))
            }
            .await;
            let _ = this.update(cx, |app, cx| {
                app.loading_account = false;
                match result {
                    Ok((index, credential)) => {
                        app.reset_account_context();
                        app.accounts = index;
                        app.credentials = Some(credential);
                        app.problem = None;
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
        if self.busy() || !self.accounts.emails.contains(&email) {
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
                .on_ok(move |_, _, cx| {
                    view.update(cx, |app, cx| app.remove_account(account.clone(), cx));
                    true
                })
        });
    }

    fn remove_account(&mut self, email: String, cx: &mut Context<Self>) {
        if self.busy() || !self.accounts.emails.contains(&email) {
            return;
        }
        self.loading_account = true;
        let original = self.accounts.clone();
        let was_active = original.active.as_deref() == Some(email.as_str());
        let mut next = original.clone();
        next.remove(&email);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result: Result<accounts::AccountIndex, String> = async {
                let saved = next.clone();
                cx.background_spawn(async move { saved.save() }).await?;
                let key = accounts::credential_key(&email);
                if let Err(error) = cx.update(|app| app.delete_credentials(&key)).await {
                    let _ = cx.background_spawn(async move { original.save() }).await;
                    return Err(format!("无法删除账号凭据：{error}"));
                }
                Ok(next)
            }
            .await;
            let _ = this.update(cx, |app, cx| {
                app.loading_account = false;
                match result {
                    Ok(index) => {
                        app.accounts = index;
                        if was_active {
                            app.credentials = None;
                            app.problem = None;
                            app.auth_open = false;
                            app.reset_account_context();
                            if app.accounts.active.is_some() {
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
}
