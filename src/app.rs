//! A task-focused GPUI shell. Only controls relevant to the current step are shown.
use crate::{
    BeginBackup, ImportAccount, OpenFiles, OpenFolders, accounts,
    backup::{self, Credentials, ItemOutcome, PauseControl, Phase},
    queue::{self, MediaFile, Queue},
    session, settings,
};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{
    App, AppContext, ClickEvent, Context, Entity, ExternalPaths, FocusHandle, Focusable, Window,
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

const BACKGROUND: u32 = 0xF7F7F5;
const SURFACE: u32 = 0xFFFFFF;
const INK: u32 = 0x20211F;
const MUTED: u32 = 0x6E726B;
const HAIRLINE: u32 = 0xE4E6E1;
const ACCENT: u32 = 0x313C34;
const ACCENT_TINT: u32 = 0xEEF1EC;
const SUCCESS: u32 = 0x27805C;
const DANGER: u32 = 0xBD554D;
const PAGE_SIZE: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Connect,
    Authorize,
    Problem,
    Empty,
    Review,
    Uploading,
    Results,
    Details,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskState {
    Idle,
    Running,
    Pausing(u64),
    Paused(u64),
    Stopping,
    Done,
}

impl TaskState {
    fn is_running(self) -> bool {
        matches!(
            self,
            Self::Running | Self::Pausing(_) | Self::Paused(_) | Self::Stopping
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResultFilter {
    All,
    Uploaded,
    Exists,
    Failed,
    Uncertain,
    Pending,
}

impl ResultFilter {
    fn matches(self, outcome: &ItemOutcome) -> bool {
        match self {
            Self::All => true,
            Self::Uploaded => matches!(outcome, ItemOutcome::Uploaded),
            Self::Exists => matches!(outcome, ItemOutcome::Exists),
            Self::Failed => matches!(outcome, ItemOutcome::Failed(_)),
            Self::Uncertain => matches!(outcome, ItemOutcome::Uncertain(_)),
            Self::Pending => matches!(outcome, ItemOutcome::Pending),
        }
    }
}

#[derive(Default)]
struct ResultCounts {
    uploaded: usize,
    exists: usize,
    failed: usize,
    uncertain: usize,
    pending: usize,
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

#[derive(Clone, Copy)]
enum ProblemKind {
    Network,
    Authorization,
    Other,
}

struct Problem {
    kind: ProblemKind,
    message: String,
}

fn classify_problem(message: String) -> Problem {
    let lower = message.to_lowercase();
    let kind = if lower.contains("网络")
        || lower.contains("请求失败")
        || lower.contains("传输失败")
        || lower.contains("连接 google 认证服务失败")
        || lower.contains("timeout")
        || lower.contains("dns")
    {
        ProblemKind::Network
    } else if lower.contains("认证")
        || lower.contains("授权")
        || lower.contains("http 401")
        || lower.contains("http 403")
    {
        ProblemKind::Authorization
    } else {
        ProblemKind::Other
    };
    Problem { kind, message }
}

pub struct UploadApp {
    focus: FocusHandle,
    app_icon: Arc<gpui_kit::Image>,
    token_input: Entity<InputState>,
    _input_subscription: gpui_kit::Subscription,
    album_input: Entity<InputState>,
    _album_subscription: gpui_kit::Subscription,
    album_text: String,
    album_target: Option<backup::AlbumTarget>,
    album_error: Option<String>,
    upload_threads: usize,
    queue: Queue,
    credentials: Option<Credentials>,
    accounts: accounts::AccountIndex,
    clear_album_input: bool,
    scanning: bool,
    authenticating: bool,
    loading_account: bool,
    auth_open: bool,
    show_auth_help: bool,
    auth_error: Option<Problem>,
    clear_auth_input: bool,
    problem: Option<Problem>,
    auth_token: String,
    transfers: HashMap<PathBuf, (Phase, u64, u64)>,
    show_details: bool,
    dark: bool,
    theme_subscription: Option<gpui_kit::Subscription>,
    result_filter: ResultFilter,
    result_page: usize,
    outcomes: HashMap<PathBuf, ItemOutcome>,
    thumbnails: HashMap<PathBuf, Thumbnail>,
    thumbnail_sequence: u64,
    run_total: usize,
    result_bytes: u64,
    task: TaskState,
    task_error: Option<String>,
    was_cancelled: bool,
    page: usize,
    undo: Option<UndoChange>,
    cancel: Arc<AtomicBool>,
    paused: Arc<PauseControl>,
    notice: String,
    current: String,
    current_path: Option<PathBuf>,
    phase: Option<Phase>,
    processed: usize,
    transfer: (u64, u64),
    errors: Vec<String>,
}

impl Focusable for UploadApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl UploadApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let token_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("粘贴 oauth_token")
                .masked(true)
        });
        let input_subscription =
            cx.subscribe_in(&token_input, window, |app, input, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    app.auth_token = input.read(cx).value().to_string();
                    app.auth_error = None;
                    app.notice.clear();
                    cx.notify();
                }
            });
        let album_input = cx
            .new(|cx| InputState::new(window, cx).placeholder("相册链接、ID 或新相册名称（可选）"));
        let album_subscription =
            cx.subscribe_in(&album_input, window, |app, input, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    app.album_text = input.read(cx).value().to_string();
                    app.album_target = None;
                    app.album_error = None;
                    cx.notify();
                }
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
            album_text: String::new(),
            album_target: None,
            album_error: None,
            upload_threads: settings::load(),
            queue: Queue::default(),
            credentials: None,
            accounts: accounts::AccountIndex::default(),
            clear_album_input: false,
            scanning: false,
            authenticating: false,
            loading_account: true,
            auth_open: false,
            show_auth_help: false,
            auth_error: None,
            clear_auth_input: false,
            problem: None,
            auth_token: String::new(),
            transfers: HashMap::new(),
            show_details: false,
            dark: false,
            theme_subscription: None,
            result_filter: ResultFilter::All,
            result_page: 0,
            outcomes: HashMap::new(),
            thumbnails: HashMap::new(),
            thumbnail_sequence: 0,
            run_total: 0,
            result_bytes: 0,
            task: TaskState::Idle,
            task_error: None,
            was_cancelled: false,
            page: 0,
            undo: None,
            cancel: Arc::new(AtomicBool::new(false)),
            paused: Arc::new(PauseControl::default()),
            notice: String::new(),
            current: String::new(),
            current_path: None,
            phase: None,
            processed: 0,
            transfer: (0, 0),
            errors: Vec::new(),
        };
        app.initialize(cx);
        app
    }

    fn screen(&self) -> Screen {
        if self.problem.is_some() {
            return Screen::Problem;
        }
        if self.auth_open {
            return Screen::Authorize;
        }
        if self.credentials.is_none() {
            return Screen::Connect;
        }
        if self.show_details && self.task == TaskState::Done {
            return Screen::Details;
        }
        if self.task.is_running() {
            Screen::Uploading
        } else if self.task == TaskState::Done {
            Screen::Results
        } else if self.queue.files.is_empty() {
            Screen::Empty
        } else {
            Screen::Review
        }
    }

    fn busy(&self) -> bool {
        self.scanning || self.authenticating || self.loading_account || self.task.is_running()
    }

    fn open_files(&mut self, _: &OpenFiles, _: &mut Window, cx: &mut Context<Self>) {
        self.choose(false, cx);
    }

    fn open_folders(&mut self, _: &OpenFolders, _: &mut Window, cx: &mut Context<Self>) {
        self.choose(true, cx);
    }

    fn begin_backup(&mut self, _: &BeginBackup, _: &mut Window, cx: &mut Context<Self>) {
        self.continue_action(cx);
    }

    fn choose_files(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.choose(false, cx);
    }

    fn choose_folders(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.choose(true, cx);
    }

    fn choose(&mut self, folders: bool, cx: &mut Context<Self>) {
        if self.busy() || !matches!(self.screen(), Screen::Empty | Screen::Review) {
            return;
        }
        self.scanning = true;
        self.notice = "正在选择来源…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let selected = cx
                .background_spawn(async move {
                    let dialog = rfd::FileDialog::new().set_title(if folders {
                        "选择需要备份的文件夹"
                    } else {
                        "选择需要备份的照片和视频"
                    });
                    if folders {
                        dialog.pick_folders()
                    } else {
                        dialog.pick_files()
                    }
                })
                .await;
            let _ = this.update(cx, |app, cx| {
                // Release the dialog guard before starting the scan.
                app.scanning = false;
                if let Some(paths) = selected {
                    app.add_paths(paths, cx);
                } else {
                    app.notice = "已取消选择".into();
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn drop_paths(&mut self, paths: &ExternalPaths, _: &mut Window, cx: &mut Context<Self>) {
        self.add_paths(paths.paths().to_vec(), cx);
    }

    fn add_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if self.busy() || paths.is_empty() {
            return;
        }
        self.scanning = true;
        self.notice = format!("正在扫描 {} 个来源…", paths.len());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_spawn(async move { queue::scan(&paths) })
                .await;
            let _ = this.update(cx, |app, cx| {
                app.scanning = false;
                let (added, ignored, warnings) = app.queue.merge(found);
                app.thumbnails.clear();
                app.task = TaskState::Idle;
                if added > 0 {
                    app.undo = None;
                }
                app.page = 0;
                app.errors.clear();
                app.reset_progress();
                app.notice = if added > 0 {
                    format!("已加入 {added} 项")
                } else {
                    "没有新增文件".into()
                };
                if ignored > 0 {
                    app.notice.push_str(&format!("，忽略 {ignored} 项"));
                }
                if !warnings.is_empty() {
                    app.notice.push_str(" · 存在扫描问题");
                    app.errors = warnings;
                }
                app.persist_queue();
                cx.notify();
            });
        })
        .detach();
    }

    fn remove_file(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        if let Some(file) = self.queue.remove(index) {
            self.undo = Some(UndoChange::File(index, file));
            self.task = TaskState::Idle;
            self.page = self
                .page
                .min(self.queue.files.len().saturating_sub(1) / PAGE_SIZE);
            self.reset_progress();
            self.notice = "已从队列移除；可撤销，本地文件不受影响".into();
            self.persist_queue();
            cx.notify();
        }
    }

    fn undo_remove(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        if let Some(change) = self.undo.take() {
            match change {
                UndoChange::File(index, file) => {
                    if self.queue.restore(index, file) {
                        self.page = index / PAGE_SIZE;
                    }
                }
                UndoChange::Queue(queue) => self.queue = queue,
            }
            self.notice = "已恢复队列".into();
            self.persist_queue();
            cx.notify();
        }
    }

    fn clear_queue(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        self.undo = Some(UndoChange::Queue(std::mem::take(&mut self.queue)));
        self.thumbnails.clear();
        self.task = TaskState::Idle;
        self.page = 0;
        self.errors.clear();
        self.reset_progress();
        self.notice = "队列已清空；可撤销，本地原件保持不变".into();
        self.persist_queue();
        cx.notify();
    }

    /// 从系统凭据存储加载账号。
    fn initialize(&mut self, cx: &mut Context<Self>) {
        self.scanning = true;
        self.notice = "正在恢复队列…".into();
        self.verify_saved_account(cx);
        cx.spawn(async move |this, cx| {
            let restored = cx
                .background_spawn(async move { session::load().map(|paths| queue::scan(&paths)) })
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
                        app.errors.extend(warnings);
                    }
                    Err(error) => app.notice = format!("无法恢复队列：{error}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn set_threads(&mut self, threads: usize, cx: &mut Context<Self>) {
        if (1..=4).contains(&threads) && threads != self.upload_threads {
            self.upload_threads = threads;
            if let Err(error) = settings::save(threads) {
                self.notice = format!("保存设置失败：{error}");
            }
        }
        cx.notify();
    }

    fn persist_queue(&mut self) {
        let paths: Vec<_> = self.queue.files.iter().map(|f| f.path.clone()).collect();
        if let Err(error) = session::save(&paths) {
            self.notice = format!("队列暂未持久保存：{error}");
        }
    }

    fn import_account_hotkey(
        &mut self,
        _: &ImportAccount,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_auth(window, cx);
    }

    fn open_auth(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        self.auth_open = true;
        self.show_auth_help = false;
        self.auth_error = None;
        self.problem = None;
        self.auth_token.clear();
        self.token_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.notice.clear();
        cx.notify();
    }

    fn connect_click(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.open_auth(window, cx);
    }

    fn back_from_auth(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.authenticating {
            return;
        }
        self.auth_open = false;
        self.problem = None;
        self.auth_error = None;
        self.auth_token.clear();
        self.token_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.notice.clear();
        cx.notify();
    }

    fn open_google(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        cx.open_url("https://accounts.google.com/EmbeddedSetup");
    }

    fn toggle_auth_help(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.show_auth_help = !self.show_auth_help;
        cx.notify();
    }

    fn complete_auth(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.begin_auth(cx);
    }

    fn begin_auth(&mut self, cx: &mut Context<Self>) {
        if self.busy() || self.auth_token.is_empty() {
            return;
        }
        let token = std::mem::take(&mut self.auth_token);
        let index = self.accounts.clone();
        self.auth_error = None;
        self.authenticating = true;
        self.notice = "正在验证 Google 授权…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let (token, result) = cx
                .background_spawn(async move {
                    let result = backup::exchange_embedded(&token).and_then(backup::verify_account);
                    (token, result)
                })
                .await;
            let result: Result<(Credentials, accounts::AccountIndex), String> = async {
                let credential = result?;
                let key = accounts::credential_key(&credential.email);
                let serialized = credential.serialized();
                cx.update(|app| {
                    app.write_credentials(&key, &credential.email, serialized.as_bytes())
                })
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
            let _ = this.update(cx, |app, cx| {
                app.authenticating = false;
                match result {
                    Ok((credential, index)) => {
                        let changed = app.credentials.as_ref().is_none_or(|previous| {
                            accounts::normalized_email(&previous.email)
                                != accounts::normalized_email(&credential.email)
                        });
                        if changed {
                            app.reset_account_context();
                        }
                        app.accounts = index;
                        app.credentials = Some(credential);
                        app.auth_open = false;
                        app.clear_auth_input = true;
                        app.problem = None;
                        app.notice = "账号已连接".into();
                    }
                    Err(error) => {
                        app.auth_token = token;
                        app.auth_error = Some(classify_problem(error));
                        app.notice.clear();
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn retry_problem(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        self.problem = None;
        self.verify_saved_account(cx);
        cx.notify();
    }

    fn reauthorize(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.open_auth(window, cx);
    }
}

mod account;
mod task;
mod view;
