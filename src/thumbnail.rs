use std::{path::Path, sync::Arc};

#[cfg(target_os = "linux")]
#[path = "thumbnail/linux.rs"]
mod platform;
#[cfg(target_os = "macos")]
#[path = "thumbnail/macos.rs"]
mod platform;
#[cfg(target_os = "windows")]
#[path = "thumbnail/windows.rs"]
mod platform;

pub fn load(path: &Path) -> Result<Arc<gpui_kit::Image>, String> {
    let (format, bytes) = platform::load(path)?;
    Ok(Arc::new(gpui_kit::Image::from_bytes(format, bytes)))
}
