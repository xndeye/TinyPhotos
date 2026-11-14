//! A task-focused GPUI shell. Only controls relevant to the current step are shown.
use crate::{
    BeginBackup, ImportAccount, OpenFiles, OpenFolders, StopBackup, TogglePause, accounts,
    backup::{self, Credentials, ItemOutcome, PauseControl, Phase},
    queue::{MediaFile, Queue},
    storage,
};
use account::{AccountOperation, AccountState};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{
    App, AppContext, ClickEvent, Context, Entity, ExternalPaths, FocusHandle, Focusable, Window,
};
use results::{ResultCounts, ResultFilter};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};
use task::{BackupTask, TaskState, TransferProgress};

const PAGE_SIZE: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen<'a> {
    Connect,
    Authorize,
    Problem(&'a backup::Error),
    Empty,
    Review,
    Uploading,
    Results,
}

enum UndoChange {
    File(usize, MediaFile),
    Queue(Queue),
}

enum Thumbnail {
    Loading(u64),
    Ready(Arc<gpui_kit::Image>),
    Unavailable,
}

pub struct UploadApp {
    focus: FocusHandle,
    app_icon: Arc<gpui_kit::Image>,
    token_input: Entity<InputState>,
    _input_subscription: gpui_kit::Subscription,
    album_input: Entity<InputState>,
    _album_subscription: gpui_kit::Subscription,
    _theme_subscription: gpui_kit::Subscription,
    account: AccountState,
    backup: BackupTask,
    upload_threads: usize,
    queue: Queue,
    scanning: bool,
    thumbnails: HashMap<PathBuf, Thumbnail>,
    thumbnail_sequence: u64,
    page: usize,
    undo: Option<UndoChange>,
    notice: String,
    queue_errors: Vec<String>,
}

impl Focusable for UploadApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl UploadApp {
    pub fn new(window: &mut Window, upload_threads: usize, cx: &mut Context<Self>) -> Self {
        let token_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("粘贴 oauth_token")
                .masked(true)
        });
        let input_subscription = cx.subscribe_in(
            &token_input,
            window,
            |app, _, event, window, cx| match event {
                InputEvent::Change => {
                    app.account.auth_error = None;
                    app.notice.clear();
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => app.begin_auth(window, cx),
                _ => {}
            },
        );
        let album_input = cx
            .new(|cx| InputState::new(window, cx).placeholder("相册链接、ID 或新相册名称（可选）"));
        let album_subscription = cx.observe(&album_input, |_, _, cx| cx.notify());
        let theme_subscription = window.observe_window_appearance(|window, cx| {
            gpui_kit::component::Theme::sync_system_appearance(Some(window), cx)
        });
        let mut app = Self {
            focus: cx.focus_handle(),
            app_icon: Arc::new(gpui_kit::Image::from_bytes(
                gpui_kit::ImageFormat::Svg,
                include_bytes!("../assets/tinyphotos.svg").to_vec(),
            )),
            token_input,
            _input_subscription: input_subscription,
            album_input,
            _album_subscription: album_subscription,
            _theme_subscription: theme_subscription,
            account: AccountState::default(),
            backup: BackupTask::default(),
            upload_threads,
            queue: Queue::default(),
            scanning: false,
            thumbnails: HashMap::new(),
            thumbnail_sequence: 0,
            page: 0,
            undo: None,
            notice: String::new(),
            queue_errors: Vec::new(),
        };
        app.initialize(cx);
        app
    }

    fn screen(&self) -> Screen<'_> {
        if self.account.auth_open {
            return Screen::Authorize;
        }
        if matches!(
            self.backup.state,
            TaskState::Done | TaskState::OrganizingAlbum
        ) && self.account.credentials.is_some()
        {
            return Screen::Results;
        }
        if let Some(error) = &self.account.problem {
            return Screen::Problem(error);
        }
        if self.account.credentials.is_none() {
            return Screen::Connect;
        }
        if self.backup.state.is_uploading() {
            Screen::Uploading
        } else if self.queue.files.is_empty() {
            Screen::Empty
        } else {
            Screen::Review
        }
    }

    fn busy(&self) -> bool {
        self.scanning
            || self.account.operation != AccountOperation::Idle
            || self.backup.state.is_busy()
    }

    /// 恢复本地队列并验证已保存的账号。
    fn initialize(&mut self, cx: &mut Context<Self>) {
        self.scanning = true;
        self.notice = "正在恢复队列…".into();
        self.verify_saved_account(cx);
        cx.spawn(async move |this, cx| {
            let restored = cx
                .background_spawn(async move {
                    storage::load_queue().map(|paths| crate::queue::scan(&paths))
                })
                .await;
            let _ = this.update(cx, |app, cx| {
                app.scanning = false;
                match restored {
                    Ok(scanned) => {
                        let (added, _, warnings) = app.queue.merge(scanned);
                        if added > 0 {
                            app.notice = format!("已恢复 {added} 个文件，继续前会重新核查云端");
                        } else {
                            app.notice.clear();
                        }
                        app.queue_errors.extend(warnings);
                    }
                    Err(error) => app.notice = format!("无法恢复队列：{error}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn set_threads(&mut self, threads: usize, cx: &mut Context<Self>) {
        self.upload_threads = threads;
        if let Err(error) = storage::save_threads(threads) {
            self.notice = format!("保存设置失败：{error}");
        }
        cx.notify();
    }
}

mod account;
mod queue;
mod results;
mod task;
mod view;
