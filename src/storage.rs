use std::{fs, io::ErrorKind, path::PathBuf};

fn directory() -> Result<PathBuf, String> {
    Ok(dirs::data_local_dir()
        .ok_or("无法确定系统本地数据目录")?
        .join("TinyPhotos"))
}

pub fn read(name: &str) -> Result<Option<Vec<u8>>, String> {
    let path = directory()?.join(name);
    match fs::read(&path) {
        Ok(data) => Ok(Some(data)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("无法读取 {}：{error}", path.display())),
    }
}

pub fn write(name: &str, data: &[u8]) -> Result<(), String> {
    let directory = directory()?;
    let path = directory.join(name);
    let temporary = path.with_extension("json.new");
    fs::create_dir_all(&directory)
        .map_err(|error| format!("无法创建 {}：{error}", directory.display()))?;
    fs::write(&temporary, data)
        .map_err(|error| format!("无法写入 {}：{error}", temporary.display()))?;
    fs::rename(&temporary, &path).map_err(|error| format!("无法保存 {}：{error}", path.display()))
}

pub fn load_queue() -> Result<Vec<PathBuf>, String> {
    match read("queue.json")? {
        Some(data) => {
            serde_json::from_slice(&data).map_err(|error| format!("保存的队列格式无效：{error}"))
        }
        None => Ok(Vec::new()),
    }
}

pub fn save_queue(files: &[PathBuf]) -> Result<(), String> {
    let data = serde_json::to_vec(files).map_err(|error| error.to_string())?;
    write("queue.json", &data)
}

pub fn load_threads() -> Result<usize, String> {
    let Some(data) = read("settings.json")? else {
        return Ok(2);
    };
    let threads = serde_json::from_slice::<usize>(&data)
        .map_err(|error| format!("设置文件格式无效：{error}"))?;
    if !(1..=4).contains(&threads) {
        return Err(format!(
            "设置文件中的上传并发数必须为 1–4，当前为 {threads}"
        ));
    }
    Ok(threads)
}

pub fn save_threads(threads: usize) -> Result<(), String> {
    write("settings.json", threads.to_string().as_bytes())
}
