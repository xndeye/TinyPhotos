//! A single optional album target, resolved from the review input without network calls.

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum AlbumTarget {
    #[default]
    None,
    Existing(String),
    Create(String),
}

#[derive(Default)]
pub struct AlbumState {
    pub(crate) target: AlbumTarget,
    pub(crate) pending_keys: std::collections::HashSet<String>,
    pub(crate) error: Option<super::Error>,
}

impl AlbumState {
    pub fn record(&mut self, key: String) {
        if !matches!(self.target, AlbumTarget::None) {
            self.pending_keys.insert(key);
        }
    }

    pub fn needs_verification(&self) -> bool {
        self.error
            .as_ref()
            .is_some_and(|error| error.kind == super::ErrorKind::UncertainAlbumCreation)
    }

    pub fn has_pending(&self) -> bool {
        !matches!(self.target, AlbumTarget::None) && !self.pending_keys.is_empty()
    }

    pub fn resolve(&self, input: &str) -> Result<AlbumTarget, String> {
        if !self.needs_verification() {
            return Ok(self.target.clone());
        }
        match parse(input)? {
            AlbumTarget::Existing(key) => Ok(AlbumTarget::Existing(key)),
            _ => Err("请先在 Google Photos 核验相册，再填写现有相册链接或 ID，避免重复创建".into()),
        }
    }
}

fn valid_key(key: &str) -> bool {
    key.len() > 6
        && key.starts_with("AF1Qip")
        && key
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

pub fn parse(input: &str) -> Result<AlbumTarget, String> {
    let value = input.trim();
    if value.is_empty() {
        return Ok(AlbumTarget::None);
    }
    if value.starts_with("https://") || value.starts_with("http://") {
        let url = url::Url::parse(value).map_err(|_| "相册链接格式无效")?;
        if url.scheme() != "https" || url.host_str() != Some("photos.google.com") {
            return Err("仅支持 photos.google.com 的普通相册链接".into());
        }
        let parts: Vec<_> = url.path_segments().ok_or("相册链接路径无效")?.collect();
        let key = match parts.as_slice() {
            ["album", key] => *key,
            ["u", account, "album", key] if account.parse::<usize>().is_ok() => *key,
            _ => return Err("仅支持 /album/ID 格式的普通相册链接".into()),
        };
        return if valid_key(key) {
            Ok(AlbumTarget::Existing(key.to_owned()))
        } else {
            Err("相册链接中的 ID 格式无效".into())
        };
    }
    if value.starts_with("AF1Qip") {
        return if valid_key(value) {
            Ok(AlbumTarget::Existing(value.to_owned()))
        } else {
            Err("相册 ID 格式无效".into())
        };
    }
    if value.contains("://") || value.contains(['\n', '\r']) {
        return Err("不支持的相册链接或名称".into());
    }
    Ok(AlbumTarget::Create(value.to_owned()))
}
