use crate::{backup::STORE_URL, storage};

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
    pub fn load() -> Result<Self, String> {
        let Some(data) = storage::read("accounts.json")? else {
            return Ok(Self::default());
        };
        let value: serde_json::Value =
            serde_json::from_slice(&data).map_err(|e| format!("账号索引格式无效：{e}"))?;
        let entries = value
            .get("emails")
            .and_then(serde_json::Value::as_array)
            .ok_or("账号索引缺少账号列表")?;
        let mut emails = Vec::new();
        for entry in entries {
            let email = normalized_email(entry.as_str().ok_or("账号索引包含无效邮箱")?);
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

    pub fn remove(&mut self, email: &str) {
        let email = normalized_email(email);
        self.emails.retain(|item| item != &email);
        if self.active.as_deref() == Some(email.as_str()) {
            self.active = self.emails.first().cloned();
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let data = serde_json::to_vec(&serde_json::json!({
            "active": self.active,
            "emails": self.emails,
        }))
        .map_err(|e| e.to_string())?;
        storage::write("accounts.json", &data)
    }
}
