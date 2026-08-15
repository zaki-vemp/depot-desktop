use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default)]
    pub google_client_id: String,
    #[serde(default)]
    pub google_client_secret: String,
    #[serde(default)]
    pub one_drive_client_id: String,
    #[serde(default)]
    pub one_drive_client_secret: String,
    #[serde(default)]
    pub dropbox_client_id: String,
    #[serde(default)]
    pub dropbox_client_secret: String,
    #[serde(default)]
    pub s3_endpoint: String,
    #[serde(default)]
    pub s3_region: String,
    #[serde(default)]
    pub s3_bucket: String,
    #[serde(default)]
    pub s3_access_key_id: String,
    #[serde(default)]
    pub s3_secret_access_key: String,
    #[serde(default)]
    pub torrent_download_dir: String,
    #[serde(default)]
    pub accounts: Vec<DriveAccount>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DriveAccount {
    pub id: String,
    pub email: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PublicAccount {
    pub id: String,
    pub email: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PublicSettings {
    pub google_client_id: String,
    pub google_client_secret: String,
    pub one_drive_client_id: String,
    pub one_drive_client_secret: String,
    pub dropbox_client_id: String,
    pub dropbox_client_secret: String,
    pub s3_endpoint: String,
    pub s3_region: String,
    pub s3_bucket: String,
    pub s3_access_key_id: String,
    pub s3_secret_access_key: String,
    pub torrent_download_dir: String,
}

pub struct Inner {
    pub settings: Settings,
    pub session: Option<Arc<librqbit::Session>>,
}

pub struct AppState {
    pub app_dir: PathBuf,
    pub inner: Mutex<Inner>,
}

impl AppState {
    pub fn new(app_dir: PathBuf) -> Self {
        let _ = fs::create_dir_all(&app_dir);
        let settings = Self::load_settings(&app_dir).unwrap_or_default();
        Self {
            app_dir,
            inner: Mutex::new(Inner {
                settings,
                session: None,
            }),
        }
    }

    fn settings_path(app_dir: &PathBuf) -> PathBuf {
        app_dir.join("settings.json")
    }

    fn load_settings(app_dir: &PathBuf) -> Result<Settings, String> {
        let path = Self::settings_path(app_dir);
        if !path.exists() {
            return Ok(Settings::default());
        }
        let data = fs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&data).map_err(|e| e.to_string())
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<(), String> {
        fs::create_dir_all(&self.app_dir).map_err(|e| e.to_string())?;
        let data = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
        fs::write(Self::settings_path(&self.app_dir), data).map_err(|e| e.to_string())
    }

    pub fn cache_dir(&self) -> PathBuf {
        let dir = self.app_dir.join("cache");
        let _ = fs::create_dir_all(&dir);
        dir
    }
}

#[cfg(test)]
mod tests {
    use super::Settings;

    #[test]
    fn legacy_google_settings_keep_accounts_when_provider_fields_expand() {
        let json = r#"{
          "googleClientId": "client-id",
          "googleClientSecret": "client-secret",
          "torrentDownloadDir": "/Downloads",
          "accounts": [{
            "id": "account-1",
            "email": "person@example.com",
            "accessToken": "access",
            "refreshToken": "refresh",
            "expiresAt": 123
          }]
        }"#;

        let settings: Settings = serde_json::from_str(json).expect("legacy settings should load");
        assert_eq!(settings.accounts.len(), 1);
        assert_eq!(settings.accounts[0].email, "person@example.com");
        assert!(settings.one_drive_client_id.is_empty());
        assert!(settings.dropbox_client_id.is_empty());
        assert!(settings.s3_bucket.is_empty());
    }
}
