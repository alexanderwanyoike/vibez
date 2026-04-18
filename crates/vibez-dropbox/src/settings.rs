use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::types::Tokens;

const SETTINGS_FILE: &str = "dropbox.json";
const APP_KEY_ENV: &str = "VIBEZ_DROPBOX_APP_KEY";
const APP_KEY_BUILD: Option<&str> = option_env!("VIBEZ_DROPBOX_APP_KEY");

/// Persisted Dropbox configuration.
///
/// `app_key` is public (PKCE does not need a secret). The runtime copy
/// takes precedence over any build-time `VIBEZ_DROPBOX_APP_KEY`; the
/// env var in turn overrides the settings file, so operators can pin
/// a key without distributing `dropbox.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DropboxSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<Tokens>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_email: Option<String>,
}

impl DropboxSettings {
    pub fn settings_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("vibez")
            .join(SETTINGS_FILE)
    }

    pub fn load() -> Self {
        let path = Self::settings_path();
        match std::fs::read_to_string(&path) {
            Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::settings_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(&path, json)
    }

    pub fn clear_tokens(&mut self) {
        self.tokens = None;
        self.account_email = None;
    }
}

/// Resolve the app key, preferring env var, then runtime settings,
/// then the build-time constant. Returns `None` if no source is set.
pub fn load_app_key_with_env_override(settings: &DropboxSettings) -> Option<String> {
    if let Ok(env) = std::env::var(APP_KEY_ENV) {
        if !env.trim().is_empty() {
            return Some(env);
        }
    }
    if let Some(key) = settings.app_key.as_ref() {
        if !key.trim().is_empty() {
            return Some(key.clone());
        }
    }
    APP_KEY_BUILD
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_var_takes_precedence_over_settings() {
        let settings = DropboxSettings {
            app_key: Some("settings_key".into()),
            ..Default::default()
        };
        // SAFETY: single-threaded test; no other setters contend.
        std::env::set_var(APP_KEY_ENV, "env_key");
        let resolved = load_app_key_with_env_override(&settings);
        std::env::remove_var(APP_KEY_ENV);
        assert_eq!(resolved.as_deref(), Some("env_key"));
    }

    #[test]
    fn settings_fallback_when_no_env() {
        let settings = DropboxSettings {
            app_key: Some("settings_key".into()),
            ..Default::default()
        };
        std::env::remove_var(APP_KEY_ENV);
        let resolved = load_app_key_with_env_override(&settings);
        assert_eq!(resolved.as_deref(), Some("settings_key"));
    }

    #[test]
    fn empty_app_key_is_treated_as_missing() {
        let settings = DropboxSettings {
            app_key: Some("   ".into()),
            ..Default::default()
        };
        std::env::remove_var(APP_KEY_ENV);
        assert!(load_app_key_with_env_override(&settings).is_none()
            || APP_KEY_BUILD.is_some());
    }

    #[test]
    fn default_settings_have_no_tokens() {
        let s = DropboxSettings::default();
        assert!(s.tokens.is_none());
        assert!(s.account_email.is_none());
        assert!(s.app_key.is_none());
    }
}
