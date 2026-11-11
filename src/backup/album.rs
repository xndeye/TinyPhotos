//! A single optional album target, resolved from the review input without network calls.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlbumTarget {
    None,
    Existing(String),
    Create(String),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn album_targets() {
        assert_eq!(parse(" ").unwrap(), AlbumTarget::None);
        assert_eq!(
            parse("https://photos.google.com/u/0/album/AF1Qipabc123?x=1").unwrap(),
            AlbumTarget::Existing("AF1Qipabc123".into())
        );
        assert_eq!(
            parse("https://photos.google.com/album/AF1Qipabc123").unwrap(),
            AlbumTarget::Existing("AF1Qipabc123".into())
        );
        assert_eq!(
            parse("旅行 2026").unwrap(),
            AlbumTarget::Create("旅行 2026".into())
        );
        assert!(parse("https://photos.app.goo.gl/abc").is_err());
        assert!(parse("https://photos.google.com/share/AF1Qipabc123").is_err());
        assert!(parse("AF1Qip?").is_err());
    }
}
