//! Dropbox API client for the vibez sample browser.
//!
//! Provides OAuth 2.0 PKCE authentication, lazy folder listing,
//! on-disk caching of downloaded files keyed by `path_lower` + `rev`,
//! and async APIs that integrate with `iced`'s `Task::perform` pattern.
//!
//! Intentional scope boundary: this crate owns HTTP + auth + cache.
//! The UI owns decoding and playback; the audio engine owns the
//! preview channel. The crate has no iced dependency so it can be
//! unit-tested with a mock HTTP server.

pub mod api;
pub mod cache;
pub mod oauth;
pub mod settings;
pub mod types;

pub use api::DropboxClient;
pub use cache::DropboxCache;
pub use oauth::{run_flow as run_oauth_flow, BrowserOpener, SystemBrowserOpener};
pub use settings::{load_app_key_with_env_override, DropboxSettings};
pub use types::{
    AccountInfo, DropboxEntry, DropboxError, DropboxListItem, DropboxListPage, DropboxResult,
    Tokens,
};
