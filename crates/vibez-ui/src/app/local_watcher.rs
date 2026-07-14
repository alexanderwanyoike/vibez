use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use iced::futures::SinkExt;
use iced::{stream, Subscription};
use notify::{RecursiveMode, Watcher};

use crate::domains::browser::BrowserMsg;
use crate::message::{LocalRootWatchEvent, Message};

/// Maximum raw filesystem events queued between polls of the
/// consumer loop. Overflowing events are dropped and replaced by a
/// conservative all-roots rescan, so memory stays bounded under
/// sustained activity (large copies/extracts into a watched root).
const WATCH_EVENT_QUEUE_CAPACITY: usize = 512;

pub(super) fn subscription(roots: Vec<PathBuf>) -> Subscription<Message> {
    if roots.is_empty() {
        return Subscription::none();
    }

    let identity = ("local-root-watcher", roots.clone());
    let events = stream::channel(128, move |mut output| async move {
        let (sender, mut receiver) =
            tokio::sync::mpsc::channel::<notify::Result<notify::Event>>(WATCH_EVENT_QUEUE_CAPACITY);
        let overflowed = Arc::new(AtomicBool::new(false));
        let callback_overflowed = Arc::clone(&overflowed);
        let mut watcher = match notify::recommended_watcher(move |event| {
            // Never block or allocate without bound inside the notify
            // callback: on a full queue, drop the event and flag the
            // overflow so the consumer rescans every root.
            if sender.try_send(event).is_err() {
                callback_overflowed.store(true, Ordering::Relaxed);
            }
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

        while let Some(first) = receiver.recv().await {
            // Coalesce everything already queued into one batch so an
            // event burst produces a single Changed message (and thus a
            // single debounce task per root) instead of one per raw
            // filesystem event.
            let mut batch = vec![first];
            while let Ok(event) = receiver.try_recv() {
                batch.push(event);
            }
            let dropped = overflowed.swap(false, Ordering::Relaxed);
            let saw_catalog_event = dropped
                || batch.iter().any(|event| {
                    event
                        .as_ref()
                        .is_ok_and(|event| is_catalog_event(&event.kind))
                });
            let (changed, failures) = coalesce_watch_events(&roots, batch, dropped);
            if saw_catalog_event {
                refresh_root_watches(&roots, &mut watched_roots, &mut watcher, &mut output).await;
            }
            if !changed.is_empty() {
                let _ = output
                    .send(Message::Browser(BrowserMsg::LocalRootWatchEvent(
                        LocalRootWatchEvent::Changed(changed),
                    )))
                    .await;
            }
            for (affected, message) in failures {
                let _ = output
                    .send(Message::Browser(BrowserMsg::LocalRootWatchEvent(
                        LocalRootWatchEvent::Failed {
                            roots: affected,
                            message,
                        },
                    )))
                    .await;
            }
        }
    });

    Subscription::run_with_id(identity, events)
}

/// Fold a batch of raw notify events into one deduplicated list of
/// changed roots (in configured-root order) plus any watch failures.
/// `dropped` marks a queue overflow, which conservatively counts
/// every root as changed.
fn coalesce_watch_events(
    roots: &[PathBuf],
    batch: Vec<notify::Result<notify::Event>>,
    dropped: bool,
) -> (Vec<PathBuf>, Vec<(Vec<PathBuf>, String)>) {
    let mut changed: Vec<PathBuf> = if dropped { roots.to_vec() } else { Vec::new() };
    let mut failures: Vec<(Vec<PathBuf>, String)> = Vec::new();
    for event in batch {
        match event {
            Ok(event) => {
                if !is_catalog_event(&event.kind) {
                    continue;
                }
                for root in affected_roots(roots, &event.paths) {
                    if !changed.contains(&root) {
                        changed.push(root);
                    }
                }
            }
            Err(error) => {
                let affected = if error.paths.is_empty() {
                    roots.to_vec()
                } else {
                    affected_roots(roots, &error.paths)
                };
                if !affected.is_empty() {
                    failures.push((affected, error.to_string()));
                }
            }
        }
    }
    (changed, failures)
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
    fn event_bursts_coalesce_into_one_deduplicated_changed_batch() {
        let roots = vec![PathBuf::from("/samples"), PathBuf::from("/other")];
        let created = |path: &str| {
            let mut event =
                notify::Event::new(notify::EventKind::Create(notify::event::CreateKind::File));
            event.paths.push(PathBuf::from(path));
            Ok(event)
        };
        let batch = vec![
            created("/samples/a.wav"),
            created("/samples/b.wav"),
            created("/samples/.cache/hidden.wav"),
            created("/other/c.wav"),
            created("/samples/d.wav"),
        ];

        let (changed, failures) = coalesce_watch_events(&roots, batch, false);
        assert_eq!(changed, roots);
        assert!(failures.is_empty());

        // A queue overflow conservatively marks every root changed.
        let (changed, failures) = coalesce_watch_events(&roots, Vec::new(), true);
        assert_eq!(changed, roots);
        assert!(failures.is_empty());

        // Noise-only batches change nothing.
        let noise = notify::Event::new(notify::EventKind::Access(notify::event::AccessKind::Open(
            notify::event::AccessMode::Read,
        )));
        let (changed, _) = coalesce_watch_events(&roots, vec![Ok(noise)], false);
        assert!(changed.is_empty());
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
