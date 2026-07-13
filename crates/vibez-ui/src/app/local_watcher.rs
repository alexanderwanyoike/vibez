use std::collections::HashSet;
use std::path::{Path, PathBuf};

use iced::futures::SinkExt;
use iced::{stream, Subscription};
use notify::{RecursiveMode, Watcher};

use crate::domains::browser::BrowserMsg;
use crate::message::{LocalRootWatchEvent, Message};

pub(super) fn subscription(roots: Vec<PathBuf>) -> Subscription<Message> {
    if roots.is_empty() {
        return Subscription::none();
    }

    let identity = ("local-root-watcher", roots.clone());
    let events = stream::channel(128, move |mut output| async move {
        let (sender, mut receiver) =
            tokio::sync::mpsc::unbounded_channel::<notify::Result<notify::Event>>();
        let mut watcher = match notify::recommended_watcher(move |event| {
            let _ = sender.send(event);
        }) {
            Ok(watcher) => watcher,
            Err(error) => {
                let _ = output
                    .send(Message::Browser(BrowserMsg::LocalRootWatchEvent(
                        LocalRootWatchEvent::Failed {
                            roots,
                            message: error.to_string(),
                        },
                    )))
                    .await;
                std::future::pending::<()>().await;
                return;
            }
        };

        let mut watched_parents = HashSet::new();
        for parent in roots.iter().filter_map(|root| root.parent()) {
            if watched_parents.insert(parent.to_path_buf()) {
                if let Err(error) = watcher.watch(parent, RecursiveMode::NonRecursive) {
                    let affected = roots
                        .iter()
                        .filter(|root| root.parent() == Some(parent))
                        .cloned()
                        .collect();
                    let _ = output
                        .send(Message::Browser(BrowserMsg::LocalRootWatchEvent(
                            LocalRootWatchEvent::Failed {
                                roots: affected,
                                message: error.to_string(),
                            },
                        )))
                        .await;
                }
            }
        }

        let mut watched_roots = HashSet::new();
        attach_available_roots(&roots, &mut watched_roots, &mut watcher, &mut output).await;

        while let Some(event) = receiver.recv().await {
            match event {
                Ok(event) => {
                    if !is_catalog_event(&event.kind) {
                        continue;
                    }
                    let affected = affected_roots(&roots, &event.paths);
                    refresh_root_watches(&roots, &mut watched_roots, &mut watcher, &mut output)
                        .await;
                    if !affected.is_empty() {
                        let _ = output
                            .send(Message::Browser(BrowserMsg::LocalRootWatchEvent(
                                LocalRootWatchEvent::Changed(affected),
                            )))
                            .await;
                    }
                }
                Err(error) => {
                    let affected = if error.paths.is_empty() {
                        roots.clone()
                    } else {
                        affected_roots(&roots, &error.paths)
                    };
                    if !affected.is_empty() {
                        let _ = output
                            .send(Message::Browser(BrowserMsg::LocalRootWatchEvent(
                                LocalRootWatchEvent::Failed {
                                    roots: affected,
                                    message: error.to_string(),
                                },
                            )))
                            .await;
                    }
                }
            }
        }
    });

    Subscription::run_with_id(identity, events)
}

fn is_catalog_event(kind: &notify::EventKind) -> bool {
    use notify::event::ModifyKind;
    use notify::EventKind;

    matches!(
        kind,
        EventKind::Any
            | EventKind::Create(_)
            | EventKind::Remove(_)
            | EventKind::Modify(ModifyKind::Any | ModifyKind::Data(_) | ModifyKind::Name(_))
    )
}

async fn attach_available_roots(
    roots: &[PathBuf],
    watched_roots: &mut HashSet<PathBuf>,
    watcher: &mut notify::RecommendedWatcher,
    output: &mut iced::futures::channel::mpsc::Sender<Message>,
) {
    let mut watching = Vec::new();
    for root in roots {
        if !root.is_dir() || watched_roots.contains(root) {
            continue;
        }
        match watcher.watch(root, RecursiveMode::Recursive) {
            Ok(()) => {
                watched_roots.insert(root.clone());
                watching.push(root.clone());
            }
            Err(error) => {
                let _ = output
                    .send(Message::Browser(BrowserMsg::LocalRootWatchEvent(
                        LocalRootWatchEvent::Failed {
                            roots: vec![root.clone()],
                            message: error.to_string(),
                        },
                    )))
                    .await;
            }
        }
    }
    if !watching.is_empty() {
        let _ = output
            .send(Message::Browser(BrowserMsg::LocalRootWatchEvent(
                LocalRootWatchEvent::Watching(watching),
            )))
            .await;
    }
}

async fn refresh_root_watches(
    roots: &[PathBuf],
    watched_roots: &mut HashSet<PathBuf>,
    watcher: &mut notify::RecommendedWatcher,
    output: &mut iced::futures::channel::mpsc::Sender<Message>,
) {
    let unavailable: Vec<_> = watched_roots
        .iter()
        .filter(|root| !root.is_dir())
        .cloned()
        .collect();
    for root in unavailable {
        let _ = watcher.unwatch(&root);
        watched_roots.remove(&root);
    }
    attach_available_roots(roots, watched_roots, watcher, output).await;
}

pub(crate) fn affected_roots(roots: &[PathBuf], paths: &[PathBuf]) -> Vec<PathBuf> {
    roots
        .iter()
        .filter(|root| {
            paths.is_empty()
                || paths.iter().any(|path| {
                    path_affects_root(root, path) && !is_hidden_beneath_root(root, path)
                })
        })
        .cloned()
        .collect()
}

fn path_affects_root(root: &Path, path: &Path) -> bool {
    path.starts_with(root) || root.starts_with(path)
}

fn is_hidden_beneath_root(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root)
        .ok()
        .is_some_and(super::audio_tasks::path_contains_hidden_component)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    #[test]
    fn affected_roots_ignore_hidden_paths_and_unrelated_siblings() {
        let roots = vec![PathBuf::from("/samples"), PathBuf::from("/other")];
        assert_eq!(
            affected_roots(&roots, &[PathBuf::from("/samples/drums/kick.wav")]),
            vec![PathBuf::from("/samples")]
        );
        assert!(affected_roots(&roots, &[PathBuf::from("/samples/.cache/a.wav")]).is_empty());
        assert!(affected_roots(&roots, &[PathBuf::from("/sibling/a.wav")]).is_empty());
    }

    #[test]
    fn watcher_ignores_access_and_metadata_noise_from_catalog_reads() {
        use notify::event::{AccessKind, AccessMode, MetadataKind, ModifyKind};

        assert!(!is_catalog_event(&notify::EventKind::Access(
            AccessKind::Open(AccessMode::Read)
        )));
        assert!(!is_catalog_event(&notify::EventKind::Modify(
            ModifyKind::Metadata(MetadataKind::AccessTime)
        )));
        assert!(is_catalog_event(&notify::EventKind::Create(
            notify::event::CreateKind::File
        )));
        assert!(is_catalog_event(&notify::EventKind::Modify(
            ModifyKind::Name(notify::event::RenameMode::Both)
        )));
    }

    #[test]
    fn recursive_watcher_observes_create_rename_move_delete_and_bursts() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("Samples");
        let drums = root.join("Drums");
        let bass = root.join("Bass");
        fs::create_dir_all(&drums).unwrap();
        fs::create_dir_all(&bass).unwrap();
        let (sender, receiver) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(sender).unwrap();
        watcher.watch(&root, RecursiveMode::Recursive).unwrap();

        let created = drums.join("kick.wav");
        fs::write(&created, b"kick").unwrap();
        wait_for_path(&receiver, &created);

        let renamed = drums.join("kick-02.wav");
        fs::rename(&created, &renamed).unwrap();
        wait_for_path(&receiver, &renamed);

        let moved = bass.join("kick-02.wav");
        fs::rename(&renamed, &moved).unwrap();
        wait_for_path(&receiver, &moved);

        for index in 0..20 {
            fs::write(bass.join(format!("burst-{index}.wav")), []).unwrap();
        }
        wait_for_path(&receiver, &bass.join("burst-19.wav"));

        fs::remove_file(&moved).unwrap();
        wait_for_path(&receiver, &moved);

        let final_catalog = super::super::audio_tasks::scan_sample_root(&root).unwrap();
        assert_eq!(final_catalog.entries.len(), 20);
        assert!(final_catalog
            .entries
            .iter()
            .all(|entry| entry.name.starts_with("burst-")));
    }

    fn wait_for_path(receiver: &mpsc::Receiver<notify::Result<notify::Event>>, expected: &Path) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let event = receiver.recv_timeout(remaining).unwrap().unwrap();
            if event.paths.iter().any(|path| path == expected) {
                return;
            }
        }
        panic!("watcher did not report {}", expected.display());
    }
}
