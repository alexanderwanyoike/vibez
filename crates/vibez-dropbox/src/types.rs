use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime};

pub type DropboxResult<T> = Result<T, DropboxError>;

/// All error paths exposed by the Dropbox client.
#[derive(Debug, thiserror::Error)]
pub enum DropboxError {
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Dropbox API returned {status}: {body}")]
    Api { status: u16, body: String },

    #[error("not authenticated: please connect to Dropbox first")]
    NotAuthenticated,

    #[error("OAuth flow failed: {0}")]
    Oauth(String),

    #[error("rate limited, retry after {0:?}")]
    RateLimited(Duration),

    #[error("app key is not configured")]
    MissingAppKey,
}

/// One entry returned by the Dropbox file listing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DropboxEntry {
    pub path_lower: String,
    pub path_display: String,
    pub name: String,
    pub is_folder: bool,
    pub rev: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropboxListItem {
    Entry(DropboxEntry),
    Deleted { path_lower: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropboxListPage {
    pub items: Vec<DropboxListItem>,
    pub cursor: String,
    pub has_more: bool,
}

impl DropboxEntry {
    pub fn file_extension(&self) -> Option<&str> {
        self.name.rsplit_once('.').map(|(_, ext)| ext)
    }

    /// True when the shared Vibez decoder contract advertises this extension.
    pub fn is_supported_audio(&self) -> bool {
        if self.is_folder {
            return false;
        }
        self.file_extension()
            .and_then(vibez_core::audio_format::audio_format_for_extension)
            .is_some()
    }
}

/// Minimal account info used to show the connected user in Settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountInfo {
    pub account_id: String,
    pub email: String,
    pub display_name: String,
}

/// Tokens persisted across sessions. `access_token` is short-lived;
/// `refresh_token` is the long-lived credential.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
    /// Seconds-since-epoch when `access_token` expires.
    pub expires_at_secs: u64,
}

impl Tokens {
    /// True if the access token expires within the given safety margin.
    pub fn needs_refresh(&self, margin: Duration) -> bool {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.expires_at_secs <= now.saturating_add(margin.as_secs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_supported_audio_extensions() {
        let entry = DropboxEntry {
            path_lower: "/kick.wav".into(),
            path_display: "/Kick.wav".into(),
            name: "Kick.wav".into(),
            is_folder: false,
            rev: Some("abc".into()),
            size: Some(1024),
        };
        assert!(entry.is_supported_audio());

        let m4a = DropboxEntry {
            name: "Idea.M4A".into(),
            path_lower: "/idea.m4a".into(),
            path_display: "/Idea.M4A".into(),
            ..entry.clone()
        };
        assert!(m4a.is_supported_audio());
    }

    #[test]
    fn rejects_non_audio_extensions() {
        let entry = DropboxEntry {
            path_lower: "/notes.txt".into(),
            path_display: "/notes.txt".into(),
            name: "notes.txt".into(),
            is_folder: false,
            rev: None,
            size: None,
        };
        assert!(!entry.is_supported_audio());

        let raw_aac = DropboxEntry {
            name: "unsupported.aac".into(),
            path_lower: "/unsupported.aac".into(),
            path_display: "/unsupported.aac".into(),
            ..entry.clone()
        };
        assert!(!raw_aac.is_supported_audio());
    }

    #[test]
    fn folder_is_never_audio() {
        let entry = DropboxEntry {
            path_lower: "/drums".into(),
            path_display: "/Drums".into(),
            name: "Drums".into(),
            is_folder: true,
            rev: None,
            size: None,
        };
        assert!(!entry.is_supported_audio());
    }

    #[test]
    fn tokens_needs_refresh_when_close_to_expiry() {
        let now_secs = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let about_to_expire = Tokens {
            access_token: "x".into(),
            refresh_token: "r".into(),
            expires_at_secs: now_secs + 60,
        };
        let fresh = Tokens {
            access_token: "x".into(),
            refresh_token: "r".into(),
            expires_at_secs: now_secs + 3600,
        };
        let expired = Tokens {
            access_token: "x".into(),
            refresh_token: "r".into(),
            expires_at_secs: now_secs.saturating_sub(1),
        };

        assert!(about_to_expire.needs_refresh(Duration::from_secs(300)));
        assert!(!fresh.needs_refresh(Duration::from_secs(300)));
        assert!(expired.needs_refresh(Duration::from_secs(300)));
    }
}
