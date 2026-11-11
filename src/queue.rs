//! Expand user-provided files/folders into a deduplicated upload queue.
//! The manifest never reads the entire file, and never follows directory symlinks.
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
    pub total_bytes: u64,
    keys: HashSet<PathBuf>,
}

fn key(path: &Path) -> PathBuf {
    // On Windows paths are normally case-insensitive. Canonicalization handles
    // relative paths and common aliases; failures retain an absolute-ish path.
    let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    #[cfg(target_os = "windows")]
    {
        PathBuf::from(canonical.to_string_lossy().to_lowercase())
    }
    #[cfg(not(target_os = "windows"))]
    {
        canonical
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
    let path_key = key(path);
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
    pub fn merge(&mut self, found: ScanResult) -> (usize, usize, Vec<String>) {
        let mut added = 0;
        for file in found.files {
            if self.keys.insert(file.key.clone()) {
                self.total_bytes = self.total_bytes.saturating_add(file.size);
                self.files.push(file);
                added += 1;
            }
        }
        (added, found.ignored, found.warnings)
    }

    /// Remove an item from the manifest only; never deletes anything from disk.
    pub fn remove(&mut self, index: usize) -> Option<MediaFile> {
        if index >= self.files.len() {
            return None;
        }
        let file = self.files.remove(index);
        self.total_bytes = self.total_bytes.saturating_sub(file.size);
        self.keys.remove(&file.key);
        Some(file)
    }

    /// Restore a previously removed item at its original position.
    pub fn restore(&mut self, index: usize, file: MediaFile) -> bool {
        if !self.keys.insert(file.key.clone()) {
            return false;
        }
        self.total_bytes = self.total_bytes.saturating_add(file.size);
        self.files.insert(index.min(self.files.len()), file);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_media_and_rejects_other_extensions() {
        assert!(supported(Path::new("A.JPG")));
        assert!(supported(Path::new("b.mP4")));
        assert!(!supported(Path::new("password.txt")));
    }

    #[test]
    fn scan_folder_and_explicit_file_without_duplicates() {
        let folder = std::env::temp_dir().join(format!("tinyphotos-queue-{}", std::process::id()));
        fs::create_dir_all(folder.join("nested")).unwrap();
        let photo = folder.join("nested").join("one.JPG");
        fs::write(&photo, b"test").unwrap();
        fs::write(folder.join("not-media.txt"), b"skip").unwrap();
        fs::write(folder.join("empty.jpg"), b"").unwrap();
        let found = scan(&[folder.clone(), photo.clone()]);
        assert!(found.warnings.iter().any(|m| m.contains("空文件")));
        let mut queue = Queue::default();
        assert_eq!(queue.merge(found).0, 1);
        assert_eq!(queue.files[0].size, 4);
        assert_eq!(queue.merge(scan(&[photo])).0, 0);
        assert_eq!(queue.total_bytes, 4);
        assert!(queue.remove(5).is_none());
        let removed = queue.remove(0).unwrap();
        assert_eq!(removed.size, 4);
        assert_eq!(queue.total_bytes, 0);
        assert!(queue.files.is_empty());
        let duplicate = removed.clone();
        assert!(queue.restore(0, removed));
        assert!(!queue.restore(0, duplicate));
        assert_eq!(queue.files.len(), 1);
        assert_eq!(queue.total_bytes, 4);
        let _ = fs::remove_dir_all(folder);
    }
}
