//! Direct HTTP implementation of the observed Android Photos upload flow.
//! Private endpoints can change without notice. Upload success requires a media key.

use super::{
    Error, ErrorKind, Event, TaskControl,
    album::AlbumTarget,
    auth::{Credentials, TokenProvider},
    protocol::*,
};
use crate::queue::MediaFile;
use base64::{Engine, engine::general_purpose::STANDARD};
use prost::Message;
use reqwest::{
    StatusCode,
    blocking::{Body, Client, RequestBuilder, Response},
};
use sha1::{Digest, Sha1};
use std::{
    fs::{self, File},
    io::{self, Read},
    path::Path,
    sync::{Arc, Mutex, atomic::Ordering},
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
    pub fn new(data: Credentials) -> Result<Self, Error> {
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(25))
            .timeout(Duration::from_secs(90))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| Error::new(ErrorKind::Network, format!("网络初始化失败：{e}")))?;
        Ok(Self {
            http,
            auth: TokenProvider::new(data),
        })
    }

    /// Verify both access-token exchange and the private Photos API before showing connected.
    pub fn validate(&mut self) -> Result<(), Error> {
        self.remote_key(&[0; 20]).map(|_| ())
    }

    pub(super) fn upload_one(
        &mut self,
        file: &MediaFile,
        sha1: &[u8; 20],
        control: Arc<TaskControl>,
        mut on_stage: impl FnMut(Phase),
        on_bytes: Arc<dyn Fn(u64, u64) + Send + Sync>,
    ) -> Result<Outcome, Error> {
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
        on_stage(Phase::Committing);
        // 响应丢失时服务端仍可能已提交，下次通过 SHA-1 核验。
        Ok(Outcome::Uploaded(self.commit(
            file,
            sha1,
            finalize_token,
        )?))
    }

    fn rpc_request(&mut self, url: &str, payload: Vec<u8>) -> Result<RequestBuilder, Error> {
        let bearer = self.auth.bearer(&self.http)?;
        Ok(self
            .http
            .post(url)
            .header("Authorization", format!("Bearer {bearer}"))
            .header("Content-Type", "application/x-protobuf")
            .header("Accept-Language", self.auth.language())
            .header("User-Agent", user_agent())
            .body(payload))
    }

    fn rpc_read(&mut self, url: &str, payload: Vec<u8>) -> Result<Vec<u8>, Error> {
        let response = self
            .rpc_request(url, payload)?
            .send()
            .map_err(|error| Error::new(ErrorKind::Network, format!("Google 请求失败：{error}")))?;
        response_bytes(response)
    }

    fn rpc_mutation(&mut self, url: &str, payload: Vec<u8>) -> Result<Vec<u8>, Error> {
        let response = self
            .rpc_request(url, payload)?
            .header("x-goog-ext-173412678-bin", "CgcIAhClARgC")
            .header("x-goog-ext-174067345-bin", "CgIIAg==")
            .send()
            .map_err(|error| {
                Error::new(
                    ErrorKind::UncertainCommit,
                    format!("Google 写入结果不确定：{error}"),
                )
            })?;
        let status = response.status();
        // 明确拒绝的请求可重新提交；超时和服务端错误不能证明写入未发生。
        if status.is_client_error() && status != StatusCode::REQUEST_TIMEOUT {
            return Err(http_error(status));
        }
        response_bytes(response).map_err(|error| {
            Error::new(
                ErrorKind::UncertainCommit,
                format!("Google 写入结果不确定：{error}"),
            )
        })
    }

    fn remote_key(&mut self, sha1: &[u8; 20]) -> Result<Option<String>, Error> {
        let msg = HashCheck {
            item: Some(HashQuery {
                hash: Some(HashBytes {
                    sha1: sha1.to_vec(),
                }),
                flags: Some(Empty {}),
            }),
        };
        let bytes = self.rpc_read(HASH_CHECK, msg.encode_to_vec())?;
        let found =
            HashMatches::decode(bytes.as_slice()).map_err(|e| format!("远程去重响应无效：{e}"))?;
        Ok(found.media_key().map(str::to_owned))
    }

    fn begin_upload(&mut self, sha1: &[u8; 20], size: u64) -> Result<String, Error> {
        let size = i64::try_from(size).map_err(|_| "文件过大".to_string())?;
        let msg = UploadRequest {
            first: 2,
            second: 2,
            third: 1,
            fourth: 3,
            size,
        };
        let bearer = self.auth.bearer(&self.http)?;
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
            .map_err(|e| Error::new(ErrorKind::Network, format!("申请上传会话失败：{e}")))?;
        let status = response.status();
        if !status.is_success() {
            return Err(http_error(status));
        }
        response
            .headers()
            .get("X-GUploader-UploadID")
            .ok_or("Google 未返回上传会话 ID")?
            .to_str()
            .map(str::to_owned)
            .map_err(|_| Error::from("上传会话 ID 无效"))
    }

    fn transfer(
        &mut self,
        path: &Path,
        id: &str,
        control: Arc<TaskControl>,
        size: u64,
        on_bytes: Arc<dyn Fn(u64, u64) + Send + Sync>,
    ) -> Result<(UploadFinalizeToken, [u8; 20]), Error> {
        let bearer = self.auth.bearer(&self.http)?;
        let encoded: String = url::form_urlencoded::byte_serialize(id.as_bytes()).collect();
        let url = format!("{UPLOAD}?upload_id={encoded}");
        let digest = Arc::new(Mutex::new(TransferDigest::default()));
        let cancel = control.cancel.clone();
        let reader = CancelableReader {
            file: File::open(path).map_err(|e| e.to_string())?,
            control,
            total: size,
            last_report: 0,
            on_bytes,
            digest: digest.clone(),
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
            .map_err(|error| {
                if cancel.load(Ordering::Relaxed) {
                    Error::new(ErrorKind::Cancelled, "已取消备份")
                } else {
                    Error::new(ErrorKind::Network, format!("传输失败：{error}"))
                }
            })?;
        let bytes = response_bytes(response)?;
        let token = UploadFinalizeToken::decode(bytes.as_slice())
            .map_err(|e| format!("上传结束凭证格式无效：{e}"))?;
        if token.version != 2 || token.opaque.is_empty() {
            return Err("上传结束凭证版本或内容无效".into());
        }
        let mut hash = digest.lock().map_err(|_| "校验状态异常")?;
        if hash.sent != size {
            return Err("网络传输字节数与文件大小不符；未提交".into());
        }
        let actual: [u8; 20] = hash.sha1.finalize_reset().into();
        Ok((token, actual))
    }

    fn commit(
        &mut self,
        file: &MediaFile,
        sha1: &[u8; 20],
        token: UploadFinalizeToken,
    ) -> Result<String, Error> {
        let modified = fs::metadata(&file.path)
            .map_err(|error| format!("无法读取文件信息：{error}"))?
            .modified()
            .map_err(|error| format!("无法读取文件修改时间：{error}"))?
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("文件修改时间早于 Unix 纪元：{error}"))?
            .as_secs() as i64;
        let msg = CommitRequest {
            media: Some(CommitMedia {
                token: Some(token),
                file_name: file.name.clone(),
                sha1: sha1.to_vec(),
                times: Some(CommitTimestamp {
                    modified_unix: modified,
                    field2: 46_000_000,
                }),
                quality: 3,
                field10: 1,
            }),
            device: Some(device_info()),
            capabilities: vec![1, 3],
        };
        let bytes = self.rpc_mutation(COMMIT, msg.encode_to_vec())?;
        let response = CommitResponse::decode(bytes.as_slice()).map_err(|e| {
            Error::new(
                ErrorKind::UncertainCommit,
                format!("提交结果不确定，响应格式无效：{e}"),
            )
        })?;
        let key = response.media_key().ok_or_else(|| {
            Error::new(
                ErrorKind::UncertainCommit,
                "Google 未确认生成媒体项；请重新扫描去重",
            )
        })?;
        Ok(key.to_string())
    }
    /// Archive only after uploading finishes. No automatic retry of album creation:
    /// a lost response may mean the album already exists.
    pub(super) fn archive(
        &mut self,
        target: &AlbumTarget,
        media_keys: &[String],
        mut on_created: impl FnMut(&str),
    ) -> Result<(), Error> {
        if media_keys.is_empty() {
            return Ok(());
        }
        let key = match target {
            AlbumTarget::None => return Ok(()),
            AlbumTarget::Existing(key) => key.clone(),
            AlbumTarget::Create(name) => {
                if media_keys.len() > 20_000 {
                    return Err(Error::from("单个新相册暂不支持超过 20000 项"));
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

    fn create_album(&mut self, name: &str, media_keys: &[String]) -> Result<String, Error> {
        let request = CreateAlbum {
            album_name: name.to_owned(),
            timestamp: unix_seconds()?,
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
            device_info: Some(device_info()),
        };
        let response = self
            .rpc_mutation(CREATE_ALBUM, request.encode_to_vec())
            .map_err(|error| {
                if error.kind == ErrorKind::UncertainCommit {
                    uncertain_album(error)
                } else {
                    error
                }
            })?;
        let response = CreateAlbumResponse::decode(response.as_slice())
            .map_err(|e| uncertain_album(format!("创建相册响应无效：{e}")))?;
        response
            .album
            .map(|album| album.album_media_key)
            .filter(|key| !key.is_empty())
            .ok_or_else(|| uncertain_album("Google 未返回新相册 ID"))
    }

    fn add_media_to_album(&mut self, album_key: &str, media_keys: &[String]) -> Result<(), Error> {
        let request = AddMediaToAlbum {
            media_keys: media_keys.to_vec(),
            album_media_key: album_key.to_owned(),
            field5: Some(AlbumFlag { field1: 2 }),
            device_info: Some(device_info()),
            timestamp: unix_seconds()?,
        };
        self.rpc_mutation(ADD_TO_ALBUM, request.encode_to_vec())?;
        Ok(())
    }
}

fn response_bytes(response: Response) -> Result<Vec<u8>, Error> {
    let status = response.status();
    if !status.is_success() {
        return Err(http_error(status));
    }
    response
        .bytes()
        .map(|b| b.to_vec())
        .map_err(|e| Error::new(ErrorKind::Network, format!("读取 Google 响应失败：{e}")))
}
pub(super) fn http_error(status: StatusCode) -> Error {
    let kind = match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ErrorKind::Authorization,
        StatusCode::TOO_MANY_REQUESTS => ErrorKind::RateLimited,
        _ => ErrorKind::Other,
    };
    Error::new(
        kind,
        format!("Google API HTTP {status}（接口可能已变更或账号无权限）"),
    )
}

fn uncertain_album(error: impl std::fmt::Display) -> Error {
    Error::new(
        ErrorKind::UncertainAlbumCreation,
        format!("创建相册结果不确定：{error}"),
    )
}
fn device_info() -> DeviceInfo {
    DeviceInfo {
        model: "Pixel XL".into(),
        make: "Google".into(),
        android_api_version: 28,
    }
}

fn unix_seconds() -> Result<i64, Error> {
    Ok(std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("系统时间早于 Unix 纪元：{error}"))?
        .as_secs() as i64)
}

fn user_agent() -> &'static str {
    "com.google.android.apps.photos/49029607 (Linux; U; Android 9; en-US; Pixel XL; Build/PQ2A.190205.001; Cronet/127.0.6510.5) (gzip)"
}

pub(super) fn hash_file(path: &Path, control: &TaskControl) -> Result<[u8; 20], Error> {
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

pub(super) fn wait_until_resumed(control: &TaskControl) -> Result<(), Error> {
    let mut reported = None;
    while control.paused.requested.load(Ordering::SeqCst) && !control.cancel.load(Ordering::Relaxed)
    {
        let generation = control.paused.generation.load(Ordering::SeqCst);
        if reported != Some(generation) {
            control
                .events
                .send_blocking(Event::Paused(generation))
                .map_err(|_| Error::from("无法报告暂停状态：任务界面已关闭"))?;
            reported = Some(generation);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if control.cancel.load(Ordering::Relaxed) {
        Err(Error::new(ErrorKind::Cancelled, "已取消备份"))
    } else {
        Ok(())
    }
}

#[derive(Default)]
struct TransferDigest {
    sha1: Sha1,
    sent: u64,
}

struct CancelableReader {
    file: File,
    control: Arc<TaskControl>,
    total: u64,
    last_report: u64,
    on_bytes: Arc<dyn Fn(u64, u64) + Send + Sync>,
    digest: Arc<Mutex<TransferDigest>>,
}
impl Read for CancelableReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        wait_until_resumed(&self.control).map_err(io::Error::other)?;
        let n = self.file.read(buf)?;
        let sent = {
            let mut digest = self
                .digest
                .lock()
                .map_err(|error| io::Error::other(error.to_string()))?;
            digest.sha1.update(&buf[..n]);
            digest.sent += n as u64;
            digest.sent
        };
        if sent - self.last_report >= 1024 * 1024 || n == 0 || sent >= self.total {
            (self.on_bytes)(sent, self.total);
            self.last_report = sent;
        }
        Ok(n)
    }
}
