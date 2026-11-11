//! Direct HTTP implementation of the observed Android Photos upload flow.
//! Private endpoints can change without notice. Upload success requires a media key.

use super::{
    Event, TaskControl,
    album::AlbumTarget,
    auth::{Credentials, TokenProvider},
    protocol::*,
};
use crate::queue::MediaFile;
use base64::{Engine, engine::general_purpose::STANDARD};
use prost::Message;
use reqwest::{
    StatusCode,
    blocking::{Body, Client, Response},
};
use sha1::{Digest, Sha1};
use std::{
    fs::{self, File},
    io::{self, Read},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, UNIX_EPOCH},
};

const UPLOAD: &str = "https://photos.googleapis.com/data/upload/uploadmedia/interactive";
const HASH_CHECK: &str =
    "https://photosdata-pa.googleapis.com/6439526531001121323/5084965799730810217";
const COMMIT: &str =
    "https://photosdata-pa.googleapis.com/6439526531001121323/16538846908252377752";
const CREATE_ALBUM: &str =
    "https://photosdata-pa.googleapis.com/6439526531001121323/8386163679468898444";
const ADD_TO_ALBUM: &str =
    "https://photosdata-pa.googleapis.com/6439526531001121323/484917746253879292";

#[derive(Clone)]
pub enum Outcome {
    Uploaded(String),
    Exists(String),
}

impl Outcome {
    pub fn media_key(&self) -> &str {
        match self {
            Self::Uploaded(key) | Self::Exists(key) => key,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Phase {
    Hashing,
    Checking,
    Uploading,
    Committing,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hashing => "计算校验值",
            Self::Checking => "检查云端重复项",
            Self::Uploading => "传输原始文件",
            Self::Committing => "等待服务端确认",
        }
    }
}

pub struct PhotosClient {
    http: Client,
    auth: TokenProvider,
}

impl PhotosClient {
    pub fn new(data: Credentials) -> Result<Self, String> {
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(25))
            .timeout(Duration::from_secs(90))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| format!("网络初始化失败：{e}"))?;
        Ok(Self {
            http,
            auth: TokenProvider::new(data),
        })
    }

    /// Verify both access-token exchange and the private Photos API before showing connected.
    pub fn validate(&mut self) -> Result<(), String> {
        if self.remote_key(&[0; 20])?.is_some() {
            return Err("Photos 验证返回了异常匹配".into());
        }
        Ok(())
    }

    pub(super) fn upload_one(
        &mut self,
        file: &MediaFile,
        sha1: &[u8; 20],
        control: Arc<TaskControl>,
        mut on_stage: impl FnMut(Phase),
        on_bytes: Arc<dyn Fn(u64, u64) + Send + Sync>,
    ) -> Result<Outcome, String> {
        let path = &file.path;
        let expected_size = file.size;
        wait_until_resumed(&control)?;
        on_stage(Phase::Checking);
        if let Some(key) = self.remote_key(sha1)? {
            return Ok(Outcome::Exists(key));
        }
        wait_until_resumed(&control)?;
        let metadata = fs::metadata(path).map_err(|e| e.to_string())?;
        if metadata.len() != expected_size {
            return Err("文件大小自加入队列以来已发生变化".into());
        }
        let upload_id = self.begin_upload(sha1, expected_size)?;
        wait_until_resumed(&control)?;
        on_stage(Phase::Uploading);
        let (finalize_token, streamed_sha1) =
            self.transfer(path, &upload_id, control.clone(), expected_size, on_bytes)?;
        if streamed_sha1 != *sha1 {
            return Err("上传时文件内容与校验值不一致；未提交".into());
        }
        wait_until_resumed(&control)?;
        // A changed file must not be committed with the hash of its old bytes.
        if fs::metadata(path).map_err(|e| e.to_string())?.len() != expected_size {
            return Err("上传期间文件发生变化；已取消提交".into());
        }
        on_stage(Phase::Committing);
        // Never retry a commit after an ambiguous transport result: it might
        // have succeeded even if the response was lost. Next run checks SHA-1.
        Ok(Outcome::Uploaded(self.commit(
            path,
            sha1,
            finalize_token,
        )?))
    }

    fn rpc(&mut self, url: &str, payload: Vec<u8>, commit: bool) -> Result<Vec<u8>, String> {
        let bearer = self.auth.bearer(&self.http)?.to_owned();
        let mut request = self
            .http
            .post(url)
            .header("Authorization", format!("Bearer {bearer}"))
            .header("Content-Type", "application/x-protobuf")
            .header("Accept-Language", self.auth.language())
            .header("User-Agent", user_agent())
            .body(payload);
        if commit {
            request = request
                .header("x-goog-ext-173412678-bin", "CgcIAhClARgC")
                .header("x-goog-ext-174067345-bin", "CgIIAg==");
        }
        let response = request
            .send()
            .map_err(|e| format!("Google 请求失败：{e}"))?;
        response_bytes(response)
    }

    fn remote_key(&mut self, sha1: &[u8; 20]) -> Result<Option<String>, String> {
        let msg = HashCheck {
            item: Some(HashQuery {
                hash: Some(HashBytes {
                    sha1: sha1.to_vec(),
                }),
                flags: Some(Empty {}),
            }),
        };
        let bytes = self.rpc(HASH_CHECK, msg.encode_to_vec(), false)?;
        let found =
            HashMatches::decode(bytes.as_slice()).map_err(|e| format!("远程去重响应无效：{e}"))?;
        Ok(found.media_key().map(str::to_owned))
    }

    fn begin_upload(&mut self, sha1: &[u8; 20], size: u64) -> Result<String, String> {
        let size = i64::try_from(size).map_err(|_| "文件过大".to_string())?;
        let msg = UploadRequest {
            first: 2,
            second: 2,
            third: 1,
            fourth: 3,
            size,
        };
        let bearer = self.auth.bearer(&self.http)?.to_owned();
        let response = self
            .http
            .post(UPLOAD)
            .header("Authorization", format!("Bearer {bearer}"))
            .header("Content-Type", "application/x-protobuf")
            .header("Accept-Language", self.auth.language())
            .header("User-Agent", user_agent())
            .header("X-Goog-Hash", format!("sha1={}", STANDARD.encode(sha1)))
            .header("X-Upload-Content-Length", size.to_string())
            .body(msg.encode_to_vec())
            .send()
            .map_err(|e| format!("申请上传会话失败：{e}"))?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!("申请上传会话失败：HTTP {status}"));
        }
        response
            .headers()
            .get("X-GUploader-UploadID")
            .ok_or("Google 未返回上传会话 ID".to_string())?
            .to_str()
            .map(str::to_owned)
            .map_err(|_| "上传会话 ID 无效".to_string())
    }

    fn transfer(
        &mut self,
        path: &Path,
        id: &str,
        control: Arc<TaskControl>,
        size: u64,
        on_bytes: Arc<dyn Fn(u64, u64) + Send + Sync>,
    ) -> Result<(UploadFinalizeToken, [u8; 20]), String> {
        let bearer = self.auth.bearer(&self.http)?.to_owned();
        let encoded: String = url::form_urlencoded::byte_serialize(id.as_bytes()).collect();
        let url = format!("{UPLOAD}?upload_id={encoded}");
        // Body::new streams directly from disk. No complete-file buffering.
        let digest = Arc::new(Mutex::new(Sha1::new()));
        let sent = Arc::new(AtomicU64::new(0));
        let reader = CancelableReader {
            file: File::open(path).map_err(|e| e.to_string())?,
            control,
            read: 0,
            total: size,
            last_report: 0,
            on_bytes,
            digest: digest.clone(),
            sent: sent.clone(),
        };
        let response = self
            .http
            .put(url)
            .header("Authorization", format!("Bearer {bearer}"))
            .header("Accept-Language", self.auth.language())
            .header("User-Agent", user_agent())
            .body(Body::new(reader))
            .timeout(Duration::from_secs(3600))
            .send()
            .map_err(|e| format!("传输失败：{e}"))?;
        let bytes = response_bytes(response)?;
        if sent.load(Ordering::Relaxed) != size {
            return Err("网络传输字节数与文件大小不符；未提交".into());
        }
        let token = UploadFinalizeToken::decode(bytes.as_slice())
            .map_err(|e| format!("上传结束凭证格式无效：{e}"))?;
        if token.version != 2 || token.opaque.is_empty() {
            return Err("上传结束凭证版本或内容无效".into());
        }
        let mut hash = digest.lock().map_err(|_| "校验状态异常")?;
        let actual: [u8; 20] = hash.finalize_reset().into();
        Ok((token, actual))
    }

    fn commit(
        &mut self,
        path: &Path,
        sha1: &[u8; 20],
        token: UploadFinalizeToken,
    ) -> Result<String, String> {
        let name = path
            .file_name()
            .ok_or("无效文件名")?
            .to_string_lossy()
            .into_owned();
        let modified = fs::metadata(path)
            .map_err(|e| e.to_string())?
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let model = "Pixel XL";
        let msg = CommitRequest {
            media: Some(CommitMedia {
                token: Some(token),
                file_name: name,
                sha1: sha1.to_vec(),
                times: Some(CommitTimestamp {
                    modified_unix: modified,
                    field2: 46_000_000,
                }),
                quality: 3,
                field10: 1,
            }),
            device: Some(DeviceInfo {
                model: model.into(),
                make: "Google".into(),
                android_api_version: 28,
            }),
            capabilities: vec![1, 3],
        };
        let bytes = self
            .rpc(COMMIT, msg.encode_to_vec(), true)
            .map_err(|e| format!("提交结果不确定，请先核查云端：{e}"))?;
        let response = CommitResponse::decode(bytes.as_slice())
            .map_err(|e| format!("提交结果不确定，响应格式无效：{e}"))?;
        let key = response
            .media_key()
            .ok_or("Google 未确认生成媒体项；提交结果不确定，请重新扫描去重")?;
        Ok(key.to_string())
    }
    /// Archive only after uploading finishes. No automatic retry of album creation:
    /// a lost response may mean the album already exists.
    pub(super) fn archive(
        &mut self,
        target: &AlbumTarget,
        media_keys: &[String],
        mut on_created: impl FnMut(&str),
    ) -> Result<(), String> {
        if media_keys.is_empty() || matches!(target, AlbumTarget::None) {
            return Ok(());
        }
        let key = match target {
            AlbumTarget::None => return Ok(()),
            AlbumTarget::Existing(key) => key.clone(),
            AlbumTarget::Create(name) => {
                if media_keys.len() > 20_000 {
                    return Err("单个新相册暂不支持超过 20000 项".into());
                }
                let key = self.create_album(name, &media_keys[..media_keys.len().min(500)])?;
                on_created(&key);
                for chunk in media_keys[media_keys.len().min(500)..].chunks(500) {
                    self.add_media_to_album(&key, chunk)?;
                }
                return Ok(());
            }
        };
        for chunk in media_keys.chunks(500) {
            self.add_media_to_album(&key, chunk)?;
        }
        Ok(())
    }

    fn create_album(&mut self, name: &str, media_keys: &[String]) -> Result<String, String> {
        let request = CreateAlbum {
            album_name: name.to_owned(),
            timestamp: unix_seconds(),
            field3: 1,
            media_keys: media_keys
                .iter()
                .map(|key| CreateAlbumItem {
                    item: Some(AlbumMediaKey {
                        media_key: key.clone(),
                    }),
                })
                .collect(),
            field6: Some(Empty {}),
            field7: Some(AlbumFlag { field1: 3 }),
            device_info: Some(album_device()),
        };
        let response = self
            .rpc(CREATE_ALBUM, request.encode_to_vec(), true)
            .map_err(|e| format!("创建相册结果不确定：{e}"))?;
        let response = CreateAlbumResponse::decode(response.as_slice())
            .map_err(|e| format!("创建相册响应无效：{e}"))?;
        response
            .album
            .map(|album| album.album_media_key)
            .filter(|key| !key.is_empty())
            .ok_or_else(|| "Google 未返回新相册 ID".to_owned())
    }

    fn add_media_to_album(&mut self, album_key: &str, media_keys: &[String]) -> Result<(), String> {
        let request = AddMediaToAlbum {
            media_keys: media_keys.to_vec(),
            album_media_key: album_key.to_owned(),
            field5: Some(AlbumFlag { field1: 2 }),
            device_info: Some(album_device()),
            timestamp: unix_seconds(),
        };
        self.rpc(ADD_TO_ALBUM, request.encode_to_vec(), true)?;
        Ok(())
    }
}

fn response_bytes(response: Response) -> Result<Vec<u8>, String> {
    let status = response.status();
    if !status.is_success() {
        return Err(http_error(status));
    }
    response
        .bytes()
        .map(|b| b.to_vec())
        .map_err(|e| format!("读取 Google 响应失败：{e}"))
}
fn http_error(status: StatusCode) -> String {
    format!("Google API HTTP {status}（接口可能已变更或账号无权限）")
}
fn album_device() -> DeviceInfo {
    DeviceInfo {
        model: "Pixel XL".into(),
        make: "Google".into(),
        android_api_version: 28,
    }
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn user_agent() -> &'static str {
    "com.google.android.apps.photos/49029607 (Linux; U; Android 9; en-US; Pixel XL; Build/PQ2A.190205.001; Cronet/127.0.6510.5) (gzip)"
}

pub(super) fn hash_file(path: &Path, control: &TaskControl) -> Result<[u8; 20], String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha1::new();
    let mut block = [0u8; 64 * 1024];
    loop {
        wait_until_resumed(control)?;
        let n = file.read(&mut block).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(&block[..n]);
    }
    Ok(hash.finalize().into())
}

fn check_cancel(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        Err("已取消备份".into())
    } else {
        Ok(())
    }
}

pub(super) fn wait_until_resumed(control: &TaskControl) -> Result<(), String> {
    let mut reported = None;
    while control.paused.requested.load(Ordering::SeqCst) && !control.cancel.load(Ordering::Relaxed)
    {
        let generation = control.paused.generation.load(Ordering::SeqCst);
        if reported != Some(generation) {
            control
                .events
                .send_blocking(Event::Paused(generation))
                .map_err(|_| "无法报告暂停状态：任务界面已关闭".to_owned())?;
            reported = Some(generation);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    check_cancel(&control.cancel)
}

struct CancelableReader {
    file: File,
    control: Arc<TaskControl>,
    read: u64,
    total: u64,
    last_report: u64,
    on_bytes: Arc<dyn Fn(u64, u64) + Send + Sync>,
    digest: Arc<Mutex<Sha1>>,
    sent: Arc<AtomicU64>,
}
impl Read for CancelableReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        wait_until_resumed(&self.control).map_err(io::Error::other)?;
        let n = self.file.read(buf)?;
        self.read += n as u64;
        if n != 0 {
            self.digest
                .lock()
                .map_err(|_| io::Error::other("hash state poisoned"))?
                .update(&buf[..n]);
            self.sent.store(self.read, Ordering::Relaxed);
        }
        if self.read.saturating_sub(self.last_report) >= 1024 * 1024
            || n == 0
            || self.read >= self.total
        {
            (self.on_bytes)(self.read, self.total);
            self.last_report = self.read;
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::PauseControl;
    #[test]
    fn cancellation() {
        assert!(check_cancel(&AtomicBool::new(true)).is_err());
    }
    #[test]
    fn hash_streaming() {
        let tmp = std::env::temp_dir().join(format!("tinyphotos-sha1-{}", std::process::id()));
        fs::write(&tmp, b"abc").unwrap();
        let (events, _receiver) = async_channel::bounded(1);
        let control = TaskControl {
            cancel: Arc::new(AtomicBool::new(false)),
            paused: Arc::new(PauseControl::default()),
            events,
        };
        let hash = hash_file(&tmp, &control).unwrap();
        assert_eq!(
            hash,
            [
                0xa9, 0x99, 0x3e, 0x36, 0x47, 0x06, 0x81, 0x6a, 0xba, 0x3e, 0x25, 0x71, 0x78, 0x50,
                0xc2, 0x6c, 0x9c, 0xd0, 0xd8, 0x9d
            ]
        );
        let _ = fs::remove_file(tmp);
    }
}
