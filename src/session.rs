//! Minimal crash-tolerant queue manifest. No secrets or upload responses are stored.
use std::{fs, io::ErrorKind, path::PathBuf};

fn path() -> Result<PathBuf, String> {
    let root = dirs::data_local_dir().ok_or("无法确定系统本地数据目录")?;
    Ok(root.join("TinyPhotos").join("queue.json"))
}

/// Recovered files are rescanned and remote-deduplicated before reuse.
pub fn load() -> Result<Vec<PathBuf>, String> {
    let target = path()?;
    let data = match fs::read(&target) {
        Ok(data) => data,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            let backup = target.with_extension("json.bak");
            match fs::read(&backup) {
                Ok(data) => data,
                Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
                Err(error) => {
                    return Err(format!("无法读取队列备份 {}：{error}", backup.display()));
                }
            }
        }
        Err(error) => return Err(format!("无法读取队列 {}：{error}", target.display())),
    };
    serde_json::from_slice(&data).map_err(|error| format!("保存的队列格式无效：{error}"))
}

pub fn save(files: &[PathBuf]) -> Result<(), String> {
    let target = path()?;
    let dir = target.parent().ok_or("无效状态目录")?;
    fs::create_dir_all(dir).map_err(|e| format!("无法创建状态目录：{e}"))?;
    let data = serde_json::to_vec(files).map_err(|e| e.to_string())?;
    let temp = target.with_extension("json.new");
    fs::write(&temp, data).map_err(|e| format!("无法保存队列：{e}"))?;
    // 保留旧快照，直到新快照写入成功。
    let backup = target.with_extension("json.bak");
    if target.exists() {
        if let Err(error) = fs::remove_file(&backup)
            && error.kind() != ErrorKind::NotFound
        {
            return Err(format!("无法删除旧队列备份：{error}"));
        }
        fs::rename(&target, &backup).map_err(|e| format!("无法备份队列：{e}"))?;
    }
    match fs::rename(&temp, &target) {
        Ok(()) => {
            let _ = fs::remove_file(backup);
            Ok(())
        }
        Err(error) => {
            if backup.exists() {
                fs::rename(&backup, &target).map_err(|restore| {
                    format!("无法提交队列：{error}；恢复旧队列失败：{restore}")
                })?;
            }
            Err(format!("无法提交队列：{error}"))
        }
    }
}
