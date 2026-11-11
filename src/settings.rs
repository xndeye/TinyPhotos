//! One persistent preference, intentionally independent of the queue manifest.
use std::{fs, path::PathBuf};

fn path() -> Option<PathBuf> {
    Some(
        dirs::data_local_dir()?
            .join("TinyPhotos")
            .join("settings.json"),
    )
}

pub fn load() -> usize {
    path()
        .and_then(|p| fs::read(p).ok())
        .and_then(|data| serde_json::from_slice::<usize>(&data).ok())
        .filter(|n| (1..=4).contains(n))
        .unwrap_or(2)
}

pub fn save(threads: usize) -> Result<(), String> {
    let path = path().ok_or("无法确定系统本地数据目录")?;
    fs::create_dir_all(path.parent().ok_or("设置目录无效")?).map_err(|e| e.to_string())?;
    fs::write(path, threads.to_string()).map_err(|e| e.to_string())
}
