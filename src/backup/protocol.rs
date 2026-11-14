//! Minimal observed wire structures; tag numbers match the private Photos RPCs.
//! These are deliberately scoped: unrelated Google Photos protobuf fields are omitted.

use prost::Message;

#[derive(Clone, PartialEq, Message)]
pub struct UploadRequest {
    #[prost(int32, tag = "1")]
    pub first: i32,
    #[prost(int32, tag = "2")]
    pub second: i32,
    #[prost(int32, tag = "3")]
    pub third: i32,
    #[prost(int32, tag = "4")]
    pub fourth: i32,
    #[prost(int64, tag = "7")]
    pub size: i64,
}

#[derive(Clone, PartialEq, Message)]
pub struct HashCheck {
    #[prost(message, optional, tag = "1")]
    pub item: Option<HashQuery>,
}
#[derive(Clone, PartialEq, Message)]
pub struct HashQuery {
    #[prost(message, optional, tag = "1")]
    pub hash: Option<HashBytes>,
    #[prost(message, optional, tag = "2")]
    pub flags: Option<Empty>,
}
#[derive(Clone, PartialEq, Message)]
pub struct HashBytes {
    #[prost(bytes = "vec", tag = "1")]
    pub sha1: Vec<u8>,
}
#[derive(Clone, PartialEq, Message)]
pub struct Empty {}

#[derive(Clone, PartialEq, Message)]
pub struct HashMatches {
    #[prost(message, optional, tag = "1")]
    pub item: Option<HashMatchesOuter>,
}
#[derive(Clone, PartialEq, Message)]
pub struct HashMatchesOuter {
    #[prost(message, optional, tag = "2")]
    pub item: Option<HashMatchesMiddle>,
}
#[derive(Clone, PartialEq, Message)]
pub struct HashMatchesMiddle {
    #[prost(message, optional, tag = "2")]
    pub result: Option<HashMatch>,
}
#[derive(Clone, PartialEq, Message)]
pub struct HashMatch {
    #[prost(string, tag = "1")]
    pub media_key: String,
}
impl HashMatches {
    pub fn media_key(&self) -> Option<&str> {
        let key = &self
            .item
            .as_ref()?
            .item
            .as_ref()?
            .result
            .as_ref()?
            .media_key;
        (!key.is_empty()).then_some(key)
    }
}

#[derive(Clone, PartialEq, Message)]
pub struct UploadFinalizeToken {
    #[prost(int64, tag = "1")]
    pub version: i64,
    #[prost(bytes = "vec", tag = "2")]
    pub opaque: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
pub struct CommitRequest {
    #[prost(message, optional, tag = "1")]
    pub media: Option<CommitMedia>,
    #[prost(message, optional, tag = "2")]
    pub device: Option<DeviceInfo>,
    #[prost(bytes = "vec", tag = "3")]
    pub capabilities: Vec<u8>,
}
#[derive(Clone, PartialEq, Message)]
pub struct CommitMedia {
    #[prost(message, optional, tag = "1")]
    pub token: Option<UploadFinalizeToken>,
    #[prost(string, tag = "2")]
    pub file_name: String,
    #[prost(bytes = "vec", tag = "3")]
    pub sha1: Vec<u8>,
    #[prost(message, optional, tag = "4")]
    pub times: Option<CommitTimestamp>,
    #[prost(int64, tag = "7")]
    pub quality: i64,
    #[prost(int64, tag = "10")]
    pub field10: i64,
}
#[derive(Clone, PartialEq, Message)]
pub struct CommitTimestamp {
    #[prost(int64, tag = "1")]
    pub modified_unix: i64,
    #[prost(int64, tag = "2")]
    pub field2: i64,
}
#[derive(Clone, PartialEq, Message)]
pub struct DeviceInfo {
    #[prost(string, tag = "3")]
    pub model: String,
    #[prost(string, tag = "4")]
    pub make: String,
    #[prost(int64, tag = "5")]
    pub android_api_version: i64,
}

#[derive(Clone, PartialEq, Message)]
pub struct CommitResponse {
    #[prost(message, repeated, tag = "1")]
    pub items: Vec<CommitResponseItem>,
}
#[derive(Clone, PartialEq, Message)]
pub struct CommitResponseItem {
    #[prost(message, optional, tag = "3")]
    pub result: Option<CommitResult>,
}
#[derive(Clone, PartialEq, Message)]
pub struct CommitResult {
    #[prost(string, tag = "1")]
    pub media_key: String,
}
impl CommitResponse {
    pub fn media_key(&self) -> Option<&str> {
        self.items
            .iter()
            .filter_map(|i| i.result.as_ref())
            .map(|r| r.media_key.as_str())
            .find(|key| !key.is_empty())
    }
}

#[derive(Clone, PartialEq, Message)]
pub struct CreateAlbum {
    #[prost(string, tag = "1")]
    pub album_name: String,
    #[prost(int64, tag = "2")]
    pub timestamp: i64,
    #[prost(int64, tag = "3")]
    pub field3: i64,
    #[prost(message, repeated, tag = "4")]
    pub media_keys: Vec<CreateAlbumItem>,
    #[prost(message, optional, tag = "6")]
    pub field6: Option<Empty>,
    #[prost(message, optional, tag = "7")]
    pub field7: Option<AlbumFlag>,
    #[prost(message, optional, tag = "8")]
    pub device_info: Option<DeviceInfo>,
}

#[derive(Clone, PartialEq, Message)]
pub struct CreateAlbumItem {
    #[prost(message, optional, tag = "1")]
    pub item: Option<AlbumMediaKey>,
}

#[derive(Clone, PartialEq, Message)]
pub struct AlbumMediaKey {
    #[prost(string, tag = "1")]
    pub media_key: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct AlbumFlag {
    #[prost(int64, tag = "1")]
    pub field1: i64,
}

#[derive(Clone, PartialEq, Message)]
pub struct CreateAlbumResponse {
    #[prost(message, optional, tag = "1")]
    pub album: Option<CreatedAlbum>,
}

#[derive(Clone, PartialEq, Message)]
pub struct CreatedAlbum {
    #[prost(string, tag = "1")]
    pub album_media_key: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct AddMediaToAlbum {
    #[prost(string, repeated, tag = "1")]
    pub media_keys: Vec<String>,
    #[prost(string, tag = "2")]
    pub album_media_key: String,
    #[prost(message, optional, tag = "5")]
    pub field5: Option<AlbumFlag>,
    #[prost(message, optional, tag = "6")]
    pub device_info: Option<DeviceInfo>,
    #[prost(int64, tag = "7")]
    pub timestamp: i64,
}
