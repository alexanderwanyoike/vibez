//! Provider-neutral metadata catalog for Browser Remote Connections.
//!
//! V1 ships a Dropbox adapter, but Browser state and persistence depend only
//! on this boundary. Remote media bytes remain a materialization concern.

use std::collections::HashMap;
use std::future::Future;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use vibez_dropbox::{DropboxClient, DropboxError, DropboxListItem};

pub const DROPBOX_PROVIDER_ID: &str = "dropbox";
pub const DROPBOX_CONNECTION_ID: &str = "dropbox-primary";
pub const DROPBOX_CONNECTION_NAME: &str = "Alex's Dropbox";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteCatalogEntry {
    pub provider_item_id: String,
    pub path: String,
    pub parent_path: String,
    pub name: String,
    pub is_folder: bool,
    pub revision: Option<String>,
    pub size: Option<u64>,
}

impl RemoteCatalogEntry {
    pub fn is_supported_audio(&self) -> bool {
        !self.is_folder
            && self
                .name
                .rsplit_once('.')
                .and_then(|(_, extension)| {
                    vibez_core::audio_format::audio_format_for_extension(extension)
                })
                .is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteCatalogSnapshot {
    pub provider_id: String,
    pub connection_id: String,
    pub connection_name: String,
    pub checkpoint: Option<String>,
    pub entries: Vec<RemoteCatalogEntry>,
}

impl Default for RemoteCatalogSnapshot {
    fn default() -> Self {
        Self {
            provider_id: DROPBOX_PROVIDER_ID.into(),
            connection_id: DROPBOX_CONNECTION_ID.into(),
            connection_name: DROPBOX_CONNECTION_NAME.into(),
            checkpoint: None,
            entries: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteChange {
    Upsert(RemoteCatalogEntry),
    Delete { provider_item_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemotePage {
    pub changes: Vec<RemoteChange>,
    pub checkpoint: String,
    pub has_more: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteProviderErrorKind {
    Authentication,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteProviderError {
    pub kind: RemoteProviderErrorKind,
    pub message: String,
}

type RemoteProviderFuture<'a> =
    Pin<Box<dyn Future<Output = Result<RemotePage, RemoteProviderError>> + Send + 'a>>;

/// Metadata-only provider contract. A future Drive or S3 adapter can implement
/// this without changing Browser state, search, persistence, or rendering.
pub trait RemoteProvider: Send + Sync {
    fn provider_id(&self) -> &'static str;
    fn connection_id(&self) -> &str;
    fn fetch_metadata_page<'a>(&'a self, checkpoint: Option<&'a str>) -> RemoteProviderFuture<'a>;
}

#[derive(Clone)]
pub struct DropboxRemoteProvider {
    client: DropboxClient,
}

impl DropboxRemoteProvider {
    pub fn new(client: DropboxClient) -> Self {
        Self { client }
    }
}

impl RemoteProvider for DropboxRemoteProvider {
    fn provider_id(&self) -> &'static str {
        DROPBOX_PROVIDER_ID
    }

    fn connection_id(&self) -> &str {
        DROPBOX_CONNECTION_ID
    }

    fn fetch_metadata_page<'a>(&'a self, checkpoint: Option<&'a str>) -> RemoteProviderFuture<'a> {
        Box::pin(async move {
            let page = self
                .client
                .list_folder_page("", true, checkpoint)
                .await
                .map_err(remote_error_from_dropbox)?;
            let changes = page
                .items
                .into_iter()
                .map(|item| match item {
                    DropboxListItem::Entry(entry) => {
                        let parent_path = parent_remote_path(&entry.path_lower);
                        RemoteChange::Upsert(RemoteCatalogEntry {
                            provider_item_id: entry.path_lower,
                            path: entry.path_display,
                            parent_path,
                            name: entry.name,
                            is_folder: entry.is_folder,
                            revision: entry.rev,
                            size: entry.size,
                        })
                    }
                    DropboxListItem::Deleted { path_lower } => RemoteChange::Delete {
                        provider_item_id: path_lower,
                    },
                })
                .collect();
            Ok(RemotePage {
                changes,
                checkpoint: page.cursor,
                has_more: page.has_more,
            })
        })
    }
}

fn remote_error_from_dropbox(error: DropboxError) -> RemoteProviderError {
    let kind = match &error {
        DropboxError::NotAuthenticated | DropboxError::MissingAppKey => {
            RemoteProviderErrorKind::Authentication
        }
        DropboxError::Api { status: 401, .. } => RemoteProviderErrorKind::Authentication,
        _ => RemoteProviderErrorKind::Unavailable,
    };
    RemoteProviderError {
        kind,
        message: error.to_string(),
    }
}

fn parent_remote_path(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
        .unwrap_or_default()
}

#[derive(Debug, Clone)]
pub struct RemoteRefreshResult {
    pub pages: usize,
    pub changes: Vec<RemoteChange>,
    /// Set only after the refresh reaches a complete provider checkpoint.
    pub checkpoint: Option<String>,
    pub error: Option<RemoteProviderError>,
}

pub async fn refresh_remote_catalog<P: RemoteProvider>(
    provider: &P,
    checkpoint: Option<&str>,
) -> RemoteRefreshResult {
    debug_assert_eq!(provider.provider_id(), DROPBOX_PROVIDER_ID);
    debug_assert_eq!(provider.connection_id(), DROPBOX_CONNECTION_ID);
    let mut cursor = checkpoint.map(ToOwned::to_owned);
    let mut pages = 0;
    let mut changes = Vec::new();
    loop {
        match provider.fetch_metadata_page(cursor.as_deref()).await {
            Ok(page) => {
                pages += 1;
                changes.extend(page.changes);
                cursor = Some(page.checkpoint);
                if !page.has_more {
                    return RemoteRefreshResult {
                        pages,
                        changes,
                        checkpoint: cursor,
                        error: None,
                    };
                }
            }
            Err(error) => {
                return RemoteRefreshResult {
                    pages,
                    changes,
                    checkpoint: None,
                    error: Some(error),
                };
            }
        }
    }
}

pub fn reconcile_remote_catalog(
    snapshot: &mut RemoteCatalogSnapshot,
    result: &RemoteRefreshResult,
) {
    let mut entries: HashMap<String, RemoteCatalogEntry> = snapshot
        .entries
        .drain(..)
        .map(|entry| (entry.provider_item_id.clone(), entry))
        .collect();
    for change in &result.changes {
        match change {
            RemoteChange::Upsert(entry) => {
                entries.insert(entry.provider_item_id.clone(), entry.clone());
            }
            RemoteChange::Delete { provider_item_id } => {
                entries.remove(provider_item_id);
            }
        }
    }
    snapshot.entries = entries.into_values().collect();
    snapshot.entries.sort_by(|left, right| {
        left.path
            .to_ascii_lowercase()
            .cmp(&right.path.to_ascii_lowercase())
    });
    if let Some(checkpoint) = &result.checkpoint {
        snapshot.checkpoint = Some(checkpoint.clone());
    }
}

#[derive(Debug, Clone)]
pub struct RemoteCatalogStore {
    path: PathBuf,
}

impl RemoteCatalogStore {
    pub fn for_dropbox() -> Self {
        let base = dirs::data_local_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("vibez")
            .join("remote-catalogs");
        Self {
            path: base.join(format!("{DROPBOX_CONNECTION_ID}.json")),
        }
    }

    #[cfg(test)]
    fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> Result<RemoteCatalogSnapshot, String> {
        match std::fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| format!("remote catalog is invalid: {error}")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(RemoteCatalogSnapshot::default())
            }
            Err(error) => Err(format!("could not read remote catalog: {error}")),
        }
    }

    pub fn save(&self, snapshot: &RemoteCatalogSnapshot) -> Result<(), String> {
        let parent = self
            .path
            .parent()
            .ok_or("remote catalog path has no parent")?;
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("could not create remote catalog folder: {error}"))?;
        let temporary = self.path.with_extension("json.partial");
        let bytes = serde_json::to_vec_pretty(snapshot)
            .map_err(|error| format!("could not encode remote catalog: {error}"))?;
        std::fs::write(&temporary, bytes)
            .map_err(|error| format!("could not write remote catalog: {error}"))?;
        std::fs::rename(&temporary, &self.path)
            .map_err(|error| format!("could not commit remote catalog: {error}"))
    }

    #[cfg(test)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use super::*;

    struct FakeProvider {
        pages: Mutex<VecDeque<Result<RemotePage, RemoteProviderError>>>,
    }

    impl RemoteProvider for FakeProvider {
        fn provider_id(&self) -> &'static str {
            DROPBOX_PROVIDER_ID
        }

        fn connection_id(&self) -> &str {
            DROPBOX_CONNECTION_ID
        }

        fn fetch_metadata_page<'a>(
            &'a self,
            _checkpoint: Option<&'a str>,
        ) -> RemoteProviderFuture<'a> {
            Box::pin(async move { self.pages.lock().unwrap().pop_front().unwrap() })
        }
    }

    fn entry(path: &str) -> RemoteCatalogEntry {
        RemoteCatalogEntry {
            provider_item_id: path.to_ascii_lowercase(),
            path: path.into(),
            parent_path: parent_remote_path(&path.to_ascii_lowercase()),
            name: path.rsplit('/').next().unwrap().into(),
            is_folder: false,
            revision: Some("1".into()),
            size: Some(42),
        }
    }

    #[tokio::test]
    async fn pagination_commits_only_the_final_checkpoint() {
        let provider = FakeProvider {
            pages: Mutex::new(VecDeque::from([
                Ok(RemotePage {
                    changes: vec![RemoteChange::Upsert(entry("/Megalodon/Kick.wav"))],
                    checkpoint: "page-1".into(),
                    has_more: true,
                }),
                Ok(RemotePage {
                    changes: vec![RemoteChange::Upsert(entry("/Megalodon/Snare.wav"))],
                    checkpoint: "complete".into(),
                    has_more: false,
                }),
            ])),
        };
        let result = refresh_remote_catalog(&provider, None).await;
        assert_eq!(result.pages, 2);
        assert_eq!(result.changes.len(), 2);
        assert_eq!(result.checkpoint.as_deref(), Some("complete"));
        assert!(result.error.is_none());
    }

    #[tokio::test]
    async fn partial_failure_keeps_changes_but_not_an_incomplete_checkpoint() {
        let provider = FakeProvider {
            pages: Mutex::new(VecDeque::from([
                Ok(RemotePage {
                    changes: vec![RemoteChange::Upsert(entry("/new.wav"))],
                    checkpoint: "unsafe".into(),
                    has_more: true,
                }),
                Err(RemoteProviderError {
                    kind: RemoteProviderErrorKind::Unavailable,
                    message: "offline".into(),
                }),
            ])),
        };
        let result = refresh_remote_catalog(&provider, Some("prior")).await;
        let mut snapshot = RemoteCatalogSnapshot {
            checkpoint: Some("prior".into()),
            entries: vec![entry("/old.wav")],
            ..RemoteCatalogSnapshot::default()
        };
        reconcile_remote_catalog(&mut snapshot, &result);
        assert_eq!(result.pages, 1);
        assert!(result.error.is_some());
        assert_eq!(snapshot.checkpoint.as_deref(), Some("prior"));
        assert_eq!(snapshot.entries.len(), 2);
    }

    #[tokio::test]
    async fn offline_first_page_failure_keeps_the_last_usable_catalog_exactly() {
        let provider = FakeProvider {
            pages: Mutex::new(VecDeque::from([Err(RemoteProviderError {
                kind: RemoteProviderErrorKind::Unavailable,
                message: "offline".into(),
            })])),
        };
        let mut snapshot = RemoteCatalogSnapshot {
            checkpoint: Some("stable".into()),
            entries: vec![entry("/Megalodon/Kick.wav")],
            ..RemoteCatalogSnapshot::default()
        };
        let before = snapshot.clone();
        let result = refresh_remote_catalog(&provider, snapshot.checkpoint.as_deref()).await;
        reconcile_remote_catalog(&mut snapshot, &result);
        assert_eq!(result.pages, 0);
        assert_eq!(snapshot, before);
    }

    #[test]
    fn dropbox_authentication_failures_are_visible_to_the_remote_boundary() {
        let error = remote_error_from_dropbox(DropboxError::Api {
            status: 401,
            body: "expired_access_token".into(),
        });
        assert_eq!(error.kind, RemoteProviderErrorKind::Authentication);
        assert!(error.message.contains("401"));
    }

    #[test]
    fn reconcile_is_idempotent_and_applies_deletions() {
        let mut snapshot = RemoteCatalogSnapshot {
            entries: vec![entry("/old.wav")],
            ..RemoteCatalogSnapshot::default()
        };
        let result = RemoteRefreshResult {
            pages: 1,
            changes: vec![
                RemoteChange::Delete {
                    provider_item_id: "/old.wav".into(),
                },
                RemoteChange::Upsert(entry("/new.wav")),
            ],
            checkpoint: Some("next".into()),
            error: None,
        };
        reconcile_remote_catalog(&mut snapshot, &result);
        reconcile_remote_catalog(&mut snapshot, &result);
        assert_eq!(snapshot.entries, vec![entry("/new.wav")]);
        assert_eq!(snapshot.checkpoint.as_deref(), Some("next"));
    }

    #[test]
    fn identical_names_at_distinct_source_identities_remain_distinct_entries() {
        let mut first = entry("/Megalodon/Kick.wav");
        let mut second = entry("/Shared/Kick.wav");
        first.name = "Kick.wav".into();
        second.name = "Kick.wav".into();
        let mut snapshot = RemoteCatalogSnapshot::default();
        reconcile_remote_catalog(
            &mut snapshot,
            &RemoteRefreshResult {
                pages: 1,
                changes: vec![RemoteChange::Upsert(first), RemoteChange::Upsert(second)],
                checkpoint: Some("complete".into()),
                error: None,
            },
        );
        assert_eq!(snapshot.entries.len(), 2);
        assert_ne!(
            snapshot.entries[0].provider_item_id,
            snapshot.entries[1].provider_item_id
        );
    }

    #[test]
    fn store_round_trips_and_reports_corruption_without_erasing_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = RemoteCatalogStore::at(dir.path().join("catalog.json"));
        let snapshot = RemoteCatalogSnapshot {
            entries: vec![entry("/Megalodon/Kick.wav")],
            ..RemoteCatalogSnapshot::default()
        };
        store.save(&snapshot).unwrap();
        assert_eq!(store.load().unwrap(), snapshot);
        std::fs::write(store.path(), b"not-json").unwrap();
        assert!(store.load().unwrap_err().contains("invalid"));
        assert_eq!(std::fs::read(store.path()).unwrap(), b"not-json");
    }

    #[test]
    fn support_filter_uses_the_shared_audio_matrix() {
        assert!(entry("/Idea.M4A").is_supported_audio());
        assert!(!entry("/notes.txt").is_supported_audio());
        assert!(!entry("/raw.aac").is_supported_audio());
    }
}
