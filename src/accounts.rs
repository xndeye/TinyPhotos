//! Non-secret account index. OAuth credentials remain in the OS credential store.
use std::{fs, io::ErrorKind, path::PathBuf};

use crate::backup::STORE_URL;

#[derive(Clone, Default)]
pub struct AccountIndex {
    pub active: Option<String>,
    pub emails: Vec<String>,
}

pub fn normalized_email(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

pub fn credential_key(email: &str) -> String {
    let email = normalized_email(email);
    let encoded: String = url::form_urlencoded::byte_serialize(email.as_bytes()).collect();
    format!("{STORE_URL}/{encoded}")
}

impl AccountIndex {
    fn path() -> Result<PathBuf, String> {
        Ok(dirs::data_local_dir()
            .ok_or("无法确定系统本地数据目录")?
            .join("TinyPhotos")
            .join("accounts.json"))
    }

    pub fn load() -> Result<Self, String> {
        let path = Self::path()?;
        let data = match fs::read(&path) {
            Ok(data) => data,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                match fs::read(path.with_extension("json.bak")) {
                    Ok(data) => data,
                    Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Self::default()),
                    Err(error) => return Err(format!("无法读取账号索引备份：{error}")),
                }
            }
            Err(error) => return Err(format!("无法读取账号索引：{error}")),
        };
        let value: serde_json::Value =
            serde_json::from_slice(&data).map_err(|e| format!("账号索引格式无效：{e}"))?;
        Self::from_value(&value)
    }

    fn from_value(value: &serde_json::Value) -> Result<Self, String> {
        let entries = value
            .get("emails")
            .and_then(serde_json::Value::as_array)
            .ok_or("账号索引缺少账号列表")?;
        let mut emails = Vec::new();
        for entry in entries {
            let email = normalized_email(entry.as_str().ok_or("账号索引包含无效邮箱")?);
            if !email.contains('@') || email.contains(['/', '\\', '\n', '\r']) {
                return Err("账号索引包含无效邮箱".into());
            }
            if !emails.contains(&email) {
                emails.push(email);
            }
        }
        let active = value
            .get("active")
            .and_then(serde_json::Value::as_str)
            .map(normalized_email)
            .filter(|email| emails.contains(email));
        Ok(Self { active, emails })
    }

    pub fn add(&mut self, email: &str) {
        let email = normalized_email(email);
        if !self.emails.contains(&email) {
            self.emails.push(email.clone());
        }
        self.active = Some(email);
    }

    pub fn remove(&mut self, email: &str) -> bool {
        let email = normalized_email(email);
        let Some(index) = self.emails.iter().position(|item| item == &email) else {
            return false;
        };
        self.emails.remove(index);
        if self.active.as_deref() == Some(email.as_str()) {
            self.active = self.emails.first().cloned();
        }
        true
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path()?;
        fs::create_dir_all(path.parent().ok_or("账号索引目录无效")?)
            .map_err(|e| format!("无法创建账号索引目录：{e}"))?;
        let data = serde_json::to_vec(&serde_json::json!({
            "active": self.active,
            "emails": self.emails,
        }))
        .map_err(|e| e.to_string())?;
        let temp = path.with_extension("json.new");
        let backup = path.with_extension("json.bak");
        fs::write(&temp, data).map_err(|e| format!("无法写入账号索引：{e}"))?;
        if path.exists() {
            if backup.exists() {
                fs::remove_file(&backup).map_err(|e| format!("无法清理旧账号索引：{e}"))?;
            }
            fs::rename(&path, &backup).map_err(|e| format!("无法备份账号索引：{e}"))?;
        }
        match fs::rename(&temp, &path) {
            Ok(()) => {
                let _ = fs::remove_file(backup);
                Ok(())
            }
            Err(error) => {
                if backup.exists() {
                    fs::rename(&backup, &path)
                        .map_err(|e| format!("提交账号索引失败：{error}；恢复失败：{e}"))?;
                }
                Err(format!("无法提交账号索引：{error}"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_index_normalizes_and_deduplicates() {
        let data = serde_json::json!({
            "active": " USER@EXAMPLE.COM ",
            "emails": ["USER@EXAMPLE.COM", "user@example.com", "other@example.com"],
        });
        let index = AccountIndex::from_value(&data).unwrap();
        assert_eq!(index.emails, ["user@example.com", "other@example.com"]);
        assert_eq!(index.active.as_deref(), Some("user@example.com"));
    }

    #[test]
    fn separate_credential_keys() {
        assert_eq!(
            credential_key(" User@Example.com "),
            credential_key("user@example.com")
        );
        assert_ne!(
            credential_key("user@example.com"),
            credential_key("other@example.com")
        );
    }
}
