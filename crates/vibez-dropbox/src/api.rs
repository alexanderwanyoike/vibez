//! Authenticated Dropbox API client.
//!
//! Wraps a `reqwest::Client` and an `Arc<tokio::sync::Mutex<Tokens>>`
//! so callers get transparent pre-flight refresh and 401 retry-once.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use tokio::sync::Mutex;

use crate::cache::DropboxCache;
use crate::oauth::refresh_access_token;
use crate::types::{AccountInfo, DropboxEntry, DropboxError, DropboxResult, Tokens};

const API_BASE: &str = "https://api.dropboxapi.com";
const CONTENT_BASE: &str = "https://content.dropboxapi.com";
/// How close to expiry before we refresh pre-emptively.
const REFRESH_MARGIN: Duration = Duration::from_secs(300);

#[derive(Clone)]
pub struct DropboxClient {
    app_key: String,
    http: reqwest::Client,
    tokens: Arc<Mutex<Tokens>>,
}

impl DropboxClient {
    pub fn new(app_key: String, tokens: Tokens) -> Self {
        let http = reqwest::Client::builder()
            .user_agent("vibez/0.1")
            .build()
            .expect("reqwest client builder must succeed with default settings");
        Self {
            app_key,
            http,
            tokens: Arc::new(Mutex::new(tokens)),
        }
    }

    /// Snapshot the current tokens (useful to persist to disk).
    pub async fn tokens(&self) -> Tokens {
        self.tokens.lock().await.clone()
    }

    /// Read the currently connected account's email + display name.
    pub async fn current_account(&self) -> DropboxResult<AccountInfo> {
        #[derive(Deserialize)]
        struct Raw {
            account_id: String,
            email: String,
            name: RawName,
        }
        #[derive(Deserialize)]
        struct RawName {
            display_name: String,
        }
        let raw: Raw = self
            .post_json(
                &format!("{API_BASE}/2/users/get_current_account"),
                &serde_json::Value::Null,
            )
            .await?;
        Ok(AccountInfo {
            account_id: raw.account_id,
            email: raw.email,
            display_name: raw.name.display_name,
        })
    }

    /// List the entries in a Dropbox folder. `path` is the Dropbox
    /// path: `""` means the root. Pagination is handled here; callers
    /// receive the full list.
    pub async fn list_folder(&self, path: &str) -> DropboxResult<Vec<DropboxEntry>> {
        #[derive(Deserialize)]
        struct Page {
            entries: Vec<ApiEntry>,
            cursor: String,
            has_more: bool,
        }

        let body = serde_json::json!({
            "path": path,
            "recursive": false,
            "include_deleted": false,
            "include_has_explicit_shared_members": false,
            "include_mounted_folders": true,
            "include_non_downloadable_files": false,
        });

        let mut page: Page = self
            .post_json(&format!("{API_BASE}/2/files/list_folder"), &body)
            .await?;

        let mut out: Vec<DropboxEntry> = page.entries.into_iter().map(Into::into).collect();
        while page.has_more {
            let cont_body = serde_json::json!({ "cursor": page.cursor });
            page = self
                .post_json(
                    &format!("{API_BASE}/2/files/list_folder/continue"),
                    &cont_body,
                )
                .await?;
            out.extend(page.entries.into_iter().map(Into::into));
        }

        Ok(out)
    }

    /// Download a file. Returns the raw bytes. The UI wraps this in
    /// `download_to_cache` most of the time.
    pub async fn download(&self, path_lower: &str) -> DropboxResult<Vec<u8>> {
        let access = self.ensure_fresh_access_token().await?;
        let arg = serde_json::json!({ "path": path_lower }).to_string();
        let response = self
            .http
            .post(format!("{CONTENT_BASE}/2/files/download"))
            .bearer_auth(&access)
            .header("Dropbox-API-Arg", arg.clone())
            .send()
            .await?;

        let mut response = response;
        if response.status().as_u16() == 401 {
            let access = self.force_refresh().await?;
            response = self
                .http
                .post(format!("{CONTENT_BASE}/2/files/download"))
                .bearer_auth(&access)
                .header("Dropbox-API-Arg", arg)
                .send()
                .await?;
        }

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(DropboxError::Api {
                status: status.as_u16(),
                body,
            });
        }

        let bytes = response.bytes().await?;
        Ok(bytes.to_vec())
    }

    /// Ensure a file is present in the cache, downloading if needed.
    /// Returns the local path.
    pub async fn download_to_cache(
        &self,
        entry: &DropboxEntry,
        cache: &DropboxCache,
    ) -> DropboxResult<PathBuf> {
        let rev = entry.rev.as_deref();
        if cache.is_cached(&entry.path_lower, rev) {
            return Ok(cache.path_for(&entry.path_lower, rev));
        }
        let bytes = self.download(&entry.path_lower).await?;
        let path = cache.write(&entry.path_lower, rev, &bytes)?;
        Ok(path)
    }

    // -- internal helpers ------------------------------------------------

    async fn post_json<T>(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> DropboxResult<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let access = self.ensure_fresh_access_token().await?;
        let mut response = self
            .http
            .post(url)
            .bearer_auth(&access)
            .json(body)
            .send()
            .await?;

        if response.status().as_u16() == 401 {
            let access = self.force_refresh().await?;
            response = self
                .http
                .post(url)
                .bearer_auth(&access)
                .json(body)
                .send()
                .await?;
        }

        let status = response.status();
        if status.as_u16() == 429 {
            return Err(DropboxError::RateLimited(Duration::from_secs(5)));
        }
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(DropboxError::Api {
                status: status.as_u16(),
                body,
            });
        }
        Ok(response.json().await?)
    }

    async fn ensure_fresh_access_token(&self) -> DropboxResult<String> {
        let needs_refresh = {
            let tokens = self.tokens.lock().await;
            tokens.needs_refresh(REFRESH_MARGIN)
        };
        if needs_refresh {
            self.force_refresh().await
        } else {
            Ok(self.tokens.lock().await.access_token.clone())
        }
    }

    async fn force_refresh(&self) -> DropboxResult<String> {
        let existing = self.tokens.lock().await.refresh_token.clone();
        let new_tokens = refresh_access_token(&self.app_key, &existing).await?;
        let access = new_tokens.access_token.clone();
        *self.tokens.lock().await = new_tokens;
        Ok(access)
    }
}

#[derive(Deserialize)]
#[serde(tag = ".tag")]
enum ApiEntry {
    #[serde(rename = "file")]
    File {
        name: String,
        path_lower: String,
        path_display: String,
        #[serde(default)]
        rev: Option<String>,
        #[serde(default)]
        size: Option<u64>,
    },
    #[serde(rename = "folder")]
    Folder {
        name: String,
        path_lower: String,
        path_display: String,
    },
    #[serde(other)]
    Other,
}

impl From<ApiEntry> for DropboxEntry {
    fn from(entry: ApiEntry) -> Self {
        match entry {
            ApiEntry::File {
                name,
                path_lower,
                path_display,
                rev,
                size,
            } => DropboxEntry {
                path_lower,
                path_display,
                name,
                is_folder: false,
                rev,
                size,
            },
            ApiEntry::Folder {
                name,
                path_lower,
                path_display,
            } => DropboxEntry {
                path_lower,
                path_display,
                name,
                is_folder: true,
                rev: None,
                size: None,
            },
            ApiEntry::Other => DropboxEntry {
                path_lower: String::new(),
                path_display: String::new(),
                name: String::new(),
                is_folder: false,
                rev: None,
                size: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_entry_deserializes_files() {
        let json = r#"{
            ".tag": "file",
            "name": "kick.wav",
            "path_lower": "/drums/kick.wav",
            "path_display": "/Drums/Kick.wav",
            "rev": "abc",
            "size": 1024
        }"#;
        let parsed: ApiEntry = serde_json::from_str(json).unwrap();
        let entry: DropboxEntry = parsed.into();
        assert!(!entry.is_folder);
        assert_eq!(entry.name, "kick.wav");
        assert_eq!(entry.path_lower, "/drums/kick.wav");
        assert_eq!(entry.path_display, "/Drums/Kick.wav");
        assert_eq!(entry.rev.as_deref(), Some("abc"));
        assert_eq!(entry.size, Some(1024));
    }

    #[test]
    fn api_entry_deserializes_folders() {
        let json = r#"{
            ".tag": "folder",
            "name": "Drums",
            "path_lower": "/drums",
            "path_display": "/Drums"
        }"#;
        let parsed: ApiEntry = serde_json::from_str(json).unwrap();
        let entry: DropboxEntry = parsed.into();
        assert!(entry.is_folder);
        assert_eq!(entry.name, "Drums");
        assert!(entry.rev.is_none());
    }
}
