#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

extern crate gpui_kit as gpui;

mod accounts;
mod app;
mod backup;
mod queue;
mod session;
mod settings;
mod thumbnail;

gpui_kit::actions!(
    tinyphotos,
    [OpenFiles, OpenFolders, BeginBackup, ImportAccount]
);

use gpui_kit::component::{Theme, TitleBar};
use gpui_kit::{
    App, AppContext, Bounds, Focusable, KeyBinding, WindowBounds, WindowOptions, px, size,
};

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx: &mut App| {
            gpui_kit::init(cx);
            #[cfg(target_os = "macos")]
            cx.bind_keys([
                KeyBinding::new("cmd-o", OpenFiles, Some("TinyPhotos")),
                KeyBinding::new("cmd-shift-o", OpenFolders, Some("TinyPhotos")),
                KeyBinding::new("cmd-enter", BeginBackup, Some("TinyPhotos")),
                KeyBinding::new("cmd-i", ImportAccount, Some("TinyPhotos")),
            ]);
            #[cfg(not(target_os = "macos"))]
            cx.bind_keys([
                KeyBinding::new("ctrl-o", OpenFiles, Some("TinyPhotos")),
                KeyBinding::new("ctrl-shift-o", OpenFolders, Some("TinyPhotos")),
                KeyBinding::new("ctrl-enter", BeginBackup, Some("TinyPhotos")),
                KeyBinding::new("ctrl-i", ImportAccount, Some("TinyPhotos")),
            ]);
            let bounds = Bounds::centered(None, size(px(760.0), px(640.0)), cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    app_id: Some("tinyphotos".into()),
                    ..TitleBar::window_options()
                },
                cx,
                |window, cx| {
                    window.set_window_title("TinyPhotos");
                    Theme::sync_system_appearance(Some(window), cx);
                    let view = cx.new(|cx| app::UploadApp::new(window, cx));
                    view.focus_handle(cx).focus(window, cx);
                    view
                },
            )
            .expect("cannot open TinyPhotos window");
            cx.activate(true);
        });
}
