//! Embedded Setup exchange and native Android Photos access-token refresh.
//! Only the Google endpoints receive tokens; errors never include secret values.
use reqwest::{blocking::Client, redirect::Policy};
use std::{
    collections::BTreeMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const PHOTOS_PACKAGE: &str = "com.google.android.apps.photos";
const PLAY_SIGNATURE: &str = "38918a453d07199354f8b19af05ec6562ced5788";
const PHOTOS_SIGNATURE: &str = "24bb24c05e47e0aefa68a58a766179d9b613a600";
const SERVICE: &str = "oauth2:openid https://www.googleapis.com/auth/mobileapps.native https://www.googleapis.com/auth/photos.native";
pub const STORE_URL: &str = "https://tinyphotos.local/google-photos";

#[derive(Clone)]
pub struct Credentials {
    fields: BTreeMap<String, String>,
    pub email: String,
    pub language: String,
    android_id: String,
}

impl Credentials {
    pub fn parse(raw: &str) -> Result<Self, String> {
        let fields: BTreeMap<String, String> = url::form_urlencoded::parse(raw.trim().as_bytes())
            .map(|(key, val)| (key.into_owned(), val.into_owned()))
            .collect();
        let get = |key: &str| fields.get(key).filter(|s| !s.is_empty()).cloned();
        let email = get("Email").ok_or("缺少 Google 账号")?;
        let android_id = get("androidId").ok_or("缺少 Android ID")?;
        if get("Token").is_none() {
            return Err("缺少主令牌".into());
        }
        if fields.contains_key("token_binding_alias") {
            return Err("该凭据要求设备密钥绑定，当前不支持".into());
        }
        Ok(Self {
            email,
            android_id,
            language: get("lang").unwrap_or_else(|| "en-US".into()),
            fields,
        })
    }

    pub fn serialized(&self) -> String {
        url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(self.fields.iter())
            .finish()
    }
}

/// Takes only the Embedded Setup `oauth_token` cookie value, never an account password.
/// The observed unofficial endpoint is not a supported desktop OAuth flow.
pub fn exchange_embedded(raw_token: &str) -> Result<Credentials, String> {
    let token = raw_token
        .trim()
        .strip_prefix("oauth_token=")
        .unwrap_or(raw_token.trim());
    if !(16..=8192).contains(&token.len()) || token.contains(['\r', '\n', '&']) {
        return Err("授权信息格式无效，请只复制 oauth_token 的值".into());
    }

    let android_id = format!("{:016x}", rand::random::<u64>());
    let mut fields = BTreeMap::from([
        ("accountType", "HOSTED_OR_GOOGLE".to_owned()),
        ("Email", "oauth-token@example.com".to_owned()),
        ("has_permission", "1".to_owned()),
        ("add_account", "1".to_owned()),
        ("ACCESS_TOKEN", "1".to_owned()),
        ("Token", token.to_owned()),
        ("service", "ac2dm".to_owned()),
        ("source", "android".to_owned()),
        ("androidId", android_id.clone()),
        ("device_country", "us".to_owned()),
        ("operatorCountry", "us".to_owned()),
        ("lang", "en".to_owned()),
        ("sdk_version", "17".to_owned()),
        ("google_play_services_version", "240913000".to_owned()),
        ("client_sig", PLAY_SIGNATURE.to_owned()),
        ("callerSig", PLAY_SIGNATURE.to_owned()),
        ("droidguard_results", "dummy123".to_owned()),
    ]);
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(Policy::none())
        .build()
        .map_err(|_| "无法初始化认证连接")?;
    let response = client
        .post("https://android.clients.google.com/auth")
        .header("User-Agent", "GoogleAuth/1.4")
        .header("Accept-Encoding", "identity")
        .form(&fields)
        .send()
        .map_err(|_| "连接 Google 认证服务失败，请检查网络")?;
    if !response.status().is_success() {
        return Err(format!("Google 拒绝授权：HTTP {}", response.status()));
    }
    let body = response.text().map_err(|_| "无法读取 Google 认证响应")?;
    if body.len() > 64 * 1024 {
        return Err("认证响应过大".into());
    }
    let result = parse_lines(&body);
    if let Some(reason) = result.get("Error") {
        return Err(match reason.as_str() {
            "BadAuthentication" => "授权已失效，请重新获取 oauth_token".into(),
            "NeedsBrowser" => "Google 要求重新在浏览器登录".into(),
            "MissingDroidguard" => "Google 要求设备校验，当前不支持".into(),
            _ => format!("Google 拒绝认证（{reason}）"),
        });
    }
    let master = result
        .get("Token")
        .filter(|s| !s.is_empty())
        .ok_or("Google 未返回主令牌")?;
    let email = result
        .get("Email")
        .filter(|s| s.contains('@'))
        .ok_or("Google 未返回有效账号")?;
    fields.clear();
    let credential = BTreeMap::from([
        ("androidId", android_id),
        ("app", PHOTOS_PACKAGE.to_owned()),
        ("callerPkg", PHOTOS_PACKAGE.to_owned()),
        ("callerSig", PHOTOS_SIGNATURE.to_owned()),
        ("client_sig", PHOTOS_SIGNATURE.to_owned()),
        ("device_country", "us".to_owned()),
        ("Email", email.to_owned()),
        ("google_play_services_version", "240913000".to_owned()),
        ("lang", "en_US".to_owned()),
        ("oauth2_foreground", "1".to_owned()),
        ("operatorCountry", "us".to_owned()),
        ("sdk_version", "33".to_owned()),
        ("service", SERVICE.to_owned()),
        ("source", "android".to_owned()),
        ("Token", master.to_owned()),
    ]);
    let serialized = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(credential.iter())
        .finish();
    Credentials::parse(&serialized)
}

fn parse_lines(body: &str) -> BTreeMap<String, String> {
    body.lines()
        .filter_map(|line| {
            let (key, value) = line.trim().split_once('=')?;
            Some((key.to_owned(), value.to_owned()))
        })
        .collect()
}

pub struct TokenProvider {
    data: Credentials,
    cached: Option<(String, u64)>,
}

impl TokenProvider {
    pub fn new(data: Credentials) -> Self {
        Self { data, cached: None }
    }
    pub fn bearer(&mut self, client: &Client) -> Result<&str, String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "系统时间不可用")?
            .as_secs();
        if self
            .cached
            .as_ref()
            .is_none_or(|(_, expiry)| *expiry <= now + 90)
        {
            let mut form = self.data.fields.clone();
            form.insert("app".into(), PHOTOS_PACKAGE.into());
            form.insert("callerPkg".into(), PHOTOS_PACKAGE.into());
            form.remove("it_caveat_types");
            form.remove("assertion_jwt");
            let response = client
                .post("https://android.googleapis.com/auth")
                .header("app", PHOTOS_PACKAGE)
                .header("device", &self.data.android_id)
                .header(
                    "User-Agent",
                    "GoogleAuth/1.4 (Pixel XL PQ2A.190205.001); gzip",
                )
                .form(&form)
                .send()
                .map_err(|_| "刷新访问令牌失败，请检查网络")?;
            if !response.status().is_success() {
                return Err(format!("访问令牌刷新失败：HTTP {}", response.status()));
            }
            let body = response.text().map_err(|_| "读取令牌响应失败")?;
            if body.len() > 64 * 1024 {
                return Err("令牌响应过大".into());
            }
            let parsed = parse_lines(&body);
            if parsed
                .get("TokenEncrypted")
                .is_some_and(|value| value == "1")
                || parsed.contains_key("EncryptedAuth")
            {
                return Err("Google 要求设备密钥绑定，当前不支持".into());
            }
            if let Some(error) = parsed.get("Error") {
                return Err(format!("Google 认证已失效（{error}），请重新连接"));
            }
            let access = parsed
                .get("Auth")
                .filter(|s| !s.is_empty())
                .ok_or("Google 未返回访问令牌")?
                .to_owned();
            let expiry = parsed
                .get("Expiry")
                .ok_or("Google 未返回过期时间")?
                .parse::<u64>()
                .map_err(|_| "过期时间无效")?;
            self.cached = Some((access, expiry));
        }
        Ok(&self.cached.as_ref().expect("token set").0)
    }
    pub fn language(&self) -> &str {
        &self.data.language
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_embedded_response_preserves_equals() {
        let data = parse_lines("Email=a@example.com\nToken=abc=def\n");
        assert_eq!(data["Token"], "abc=def");
    }
    #[test]
    fn credential_round_trip() {
        let c = Credentials::parse("androidId=a&Email=a%40gmail.com&Token=a%3Db").unwrap();
        let decoded = Credentials::parse(&c.serialized()).unwrap();
        assert_eq!(decoded.email, c.email);
    }
    #[test]
    fn bad_cookie_rejected() {
        assert!(exchange_embedded("short").is_err());
        assert!(exchange_embedded("1234567890123456\nInjected").is_err());
    }
}
