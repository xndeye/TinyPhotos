use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

#[derive(Clone, Debug)]
pub struct MediaFile {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    key: PathBuf,
}

#[derive(Default)]
pub struct ScanResult {
    pub files: Vec<MediaFile>,
    pub ignored: usize,
    pub warnings: Vec<String>,
}

#[derive(Default)]
pub struct Queue {
    pub files: Vec<MediaFile>,
    keys: HashSet<PathBuf>,
}

fn key(path: &Path) -> std::io::Result<PathBuf> {
    let canonical = fs::canonicalize(path)?;
    #[cfg(target_os = "windows")]
    {
        Ok(PathBuf::from(canonical.to_string_lossy().to_lowercase()))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(canonical)
    }
}

fn supported(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "avif"
            | "bmp"
            | "gif"
            | "heic"
            | "heif"
            | "ico"
            | "jpg"
            | "jpeg"
            | "png"
            | "tif"
            | "tiff"
            | "webp"
            | "dng"
            | "cr2"
            | "cr3"
            | "nef"
            | "arw"
            | "orf"
            | "raf"
            | "rw2"
            | "pef"
            | "sr2"
            | "3gp"
            | "3g2"
            | "asf"
            | "avi"
            | "divx"
            | "m2t"
            | "m2ts"
            | "m4v"
            | "mkv"
            | "mmv"
            | "mod"
            | "mov"
            | "mp4"
            | "mpg"
            | "mpeg"
            | "mts"
            | "tod"
            | "ts"
            | "webm"
            | "wmv"
    )
}

fn add_file(path: &Path, result: &mut ScanResult, seen: &mut HashSet<PathBuf>) {
    if !supported(path) {
        result.ignored += 1;
        return;
    }
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let path_key = match key(path) {
        Ok(key) => key,
        Err(error) => {
            result.warn(format!("无法定位 {}：{error}", path.display()));
            return;
        }
    };
    if seen.contains(&path_key) {
        return;
    }
    match fs::metadata(path) {
        Ok(meta) if meta.is_file() && meta.len() > 0 => {
            seen.insert(path_key.clone());
            result.files.push(MediaFile {
                path: path.to_owned(),
                name,
                size: meta.len(),
                key: path_key,
            });
        }
        Ok(meta) if meta.is_file() => {
            result.ignored += 1;
            result.warn(format!("已忽略空文件：{}", path.display()));
        }
        Ok(_) => result.ignored += 1,
        Err(err) => result.warn(format!("无法读取 {}：{err}", path.display())),
    }
}

impl ScanResult {
    fn warn(&mut self, message: String) {
        if self.warnings.len() < 8 {
            self.warnings.push(message);
        }
    }
}

pub fn scan(inputs: &[PathBuf]) -> ScanResult {
    let mut result = ScanResult::default();
    let mut seen = HashSet::new();
    for input in inputs {
        // Do not recursively dereference junctions/symlinks supplied as roots.
        let Ok(metadata) = fs::symlink_metadata(input) else {
            result.warn(format!("路径不存在：{}", input.display()));
            continue;
        };
        if metadata.file_type().is_symlink() {
            result.warn(format!("已跳过符号链接：{}", input.display()));
        } else if metadata.is_file() {
            add_file(input, &mut result, &mut seen);
        } else if metadata.is_dir() {
            for entry in WalkDir::new(input).follow_links(false) {
                match entry {
                    Ok(entry) if entry.file_type().is_file() => {
                        add_file(entry.path(), &mut result, &mut seen);
                    }
                    Ok(_) => (),
                    Err(err) => result.warn(format!("扫描目录时跳过条目：{err}")),
                }
            }
        } else {
            result.ignored += 1;
        }
    }
    result
}

impl Queue {
    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|file| file.size).sum()
    }

    pub fn merge(&mut self, found: ScanResult) -> (usize, usize, Vec<String>) {
        let mut added = 0;
        for file in found.files {
            if self.keys.insert(file.key.clone()) {
                self.files.push(file);
                added += 1;
            }
        }
        (added, found.ignored, found.warnings)
    }

    /// 仅从队列移除文件，不删除本地原件。
    pub fn remove(&mut self, index: usize) -> MediaFile {
        let file = self.files.remove(index);
        self.keys.remove(&file.key);
        file
    }

    pub fn restore(&mut self, index: usize, file: MediaFile) {
        self.keys.insert(file.key.clone());
        self.files.insert(index, file);
    }
}
