use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant, UNIX_EPOCH},
};

fn cached(path: &Path, uri: &str, cache: &Path) -> Result<Vec<u8>, String> {
    let metadata = fs::metadata(path).map_err(|error| format!("无法读取原文件信息：{error}"))?;
    let modified = metadata
        .modified()
        .map_err(|error| format!("无法读取文件修改时间：{error}"))?
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("文件修改时间无效：{error}"))?
        .as_secs();
    let bytes = fs::read(cache).map_err(|error| format!("无法读取缩略图缓存：{error}"))?;
    {
        let reader = png::Decoder::new(Cursor::new(&bytes))
            .read_info()
            .map_err(|error| format!("系统缩略图格式无效：{error}"))?;
        let text = &reader.info().uncompressed_latin1_text;
        let value = |key: &str| {
            text.iter()
                .find(|item| item.keyword == key)
                .map(|item| item.text.as_str())
        };
        let modified = modified.to_string();
        if value("Thumb::URI") != Some(uri) || value("Thumb::MTime") != Some(modified.as_str()) {
            return Err("系统缩略图已过期".into());
        }
        if value("Thumb::Size").is_some_and(|size| size.parse::<u64>().ok() != Some(metadata.len()))
        {
            return Err("系统缩略图文件大小不匹配".into());
        }
    }
    Ok(bytes)
}

pub fn load(path: &Path) -> Result<(gpui_kit::ImageFormat, Vec<u8>), String> {
    let path = fs::canonicalize(path).map_err(|error| format!("无法定位缩略图来源：{error}"))?;
    let uri = url::Url::from_file_path(&path)
        .map_err(|_| "无法转换文件 URI")?
        .to_string();
    let root = dirs::cache_dir().ok_or("无法确定系统缓存目录")?;
    let name = format!("{:x}.png", md5::compute(uri.as_bytes()));
    let caches: Vec<PathBuf> = ["normal", "large", "x-large", "xx-large"]
        .into_iter()
        .map(|flavor| root.join("thumbnails").join(flavor).join(&name))
        .collect();
    for cache in &caches {
        if let Ok(bytes) = cached(&path, &uri, cache) {
            return Ok((gpui_kit::ImageFormat::Png, bytes));
        }
    }
    let mime = mime_guess::from_path(&path)
        .first_raw()
        .ok_or("系统无法识别文件类型")?;
    let connection = zbus::blocking::Connection::session()
        .map_err(|error| format!("无法连接桌面会话：{error}"))?;
    let service = zbus::blocking::Proxy::new(
        &connection,
        "org.freedesktop.thumbnails.Thumbnailer1",
        "/org/freedesktop/thumbnails/Thumbnailer1",
        "org.freedesktop.thumbnails.Thumbnailer1",
    )
    .map_err(|error| format!("无法连接系统缩略图服务：{error}"))?;
    let handle: u32 = service
        .call(
            "Queue",
            &(vec![uri.as_str()], vec![mime], "normal", "default", 0u32),
        )
        .map_err(|error| format!("无法请求系统缩略图：{error}"))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(bytes) = cached(&path, &uri, &caches[0]) {
            return Ok((gpui_kit::ImageFormat::Png, bytes));
        }
        thread::sleep(Duration::from_millis(100));
    }
    let _: () = service
        .call("Dequeue", &(handle,))
        .map_err(|error| format!("取消系统缩略图请求失败：{error}"))?;
    Err("系统未生成可用缩略图".into())
}
