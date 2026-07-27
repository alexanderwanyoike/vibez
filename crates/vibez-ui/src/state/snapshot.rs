//! Project snapshots and bounded undo/redo history.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use super::{ArrangementTimeline, ProjectTracksState};

/// A point-in-time snapshot of the editable project state, used to implement
/// undo / redo. Project Tracks, Arrange, and the Section store are independently
/// shared so edits clone only the project-owned structure they change.
#[derive(Debug, Clone)]
pub struct ProjectSnapshot {
    pub project_tracks: Arc<ProjectTracksState>,
    pub arrange_timeline: Arc<ArrangementTimeline>,
    pub sections: Arc<crate::domains::perform::SectionStore>,
    pub bpm: f64,
    pub project_swing: vibez_core::perform::SwingAmount,
    pub loop_enabled: bool,
    pub loop_start_beats: f64,
    pub loop_end_beats: f64,
}

/// One open project edit that will either become one undo step or be rolled
/// back. The pre-edit snapshot is independently `Arc`-shared, so opening a
/// recording-length transaction does not clone project media or device state.
#[derive(Debug)]
struct ProjectTransaction {
    before: ProjectSnapshot,
    dirty_before: bool,
    changed: bool,
}

/// Runtime identity for one continuous pointer gesture. Every incremental
/// project edit emitted while the pointer remains held carries the same id so
/// undo history can retain the pre-gesture snapshot only once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UndoGestureId(u64);

impl UndoGestureId {
    pub fn new() -> Self {
        static NEXT_GESTURE_ID: AtomicU64 = AtomicU64::new(1);
        Self(NEXT_GESTURE_ID.fetch_add(1, Ordering::Relaxed))
    }
}

/// Project domain slice: file-menu visibility, the current file,
/// the dirty flag, and the undo/redo history.
#[derive(Debug, Default)]
pub struct ProjectState {
    pub file_menu_open: bool,
    pub current_path: Option<PathBuf>,
    pub dirty: bool,
    pub history: UndoHistory,
    /// Clips whose media could not be hydrated at load time. Invisible in
    /// the arrangement, but serialized back into every save so unavailable
    /// media stays relinkable instead of silently vanishing.
    pub unresolved_clips: Vec<crate::message::UnresolvedTimelineClip>,
}

#[derive(Debug, Default)]
pub struct UndoHistory {
    pub undo: VecDeque<ProjectSnapshot>,
    pub redo: VecDeque<ProjectSnapshot>,
    last_gesture: Option<UndoGestureId>,
    transaction: Option<ProjectTransaction>,
}

impl UndoHistory {
    pub const CAPACITY: usize = 100;

    pub fn push_undo(&mut self, snapshot: ProjectSnapshot) {
        self.last_gesture = None;
        self.push_snapshot(snapshot);
    }

    fn push_snapshot(&mut self, snapshot: ProjectSnapshot) {
        self.undo.push_back(snapshot);
        if self.undo.len() > Self::CAPACITY {
            self.undo.pop_front();
        }
        self.redo.clear();
    }

    pub fn push_edit(&mut self, snapshot: ProjectSnapshot, gesture: Option<UndoGestureId>) {
        if let Some(transaction) = &mut self.transaction {
            transaction.changed = true;
            self.last_gesture = None;
            return;
        }
        if gesture.is_some() && self.last_gesture == gesture {
            return;
        }
        self.push_snapshot(snapshot);
        self.last_gesture = gesture;
    }

    pub fn pop_undo(&mut self) -> Option<ProjectSnapshot> {
        self.last_gesture = None;
        self.undo.pop_back()
    }

    pub fn push_redo(&mut self, snapshot: ProjectSnapshot) {
        self.redo.push_back(snapshot);
        if self.redo.len() > Self::CAPACITY {
            self.redo.pop_front();
        }
    }

    pub fn pop_redo(&mut self) -> Option<ProjectSnapshot> {
        self.last_gesture = None;
        self.redo.pop_back()
    }

    #[allow(dead_code)]
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    #[allow(dead_code)]
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Open a transaction at the supplied pre-edit project state. Nested
    /// transactions are deliberately rejected: an inner multi-edit operation
    /// simply participates in the already-open recording transaction.
    pub fn begin_transaction(&mut self, before: ProjectSnapshot, dirty_before: bool) -> bool {
        if self.transaction.is_some() {
            return false;
        }
        self.last_gesture = None;
        self.transaction = Some(ProjectTransaction {
            before,
            dirty_before,
            changed: false,
        });
        true
    }

    /// Commit every edit since `begin_transaction` as exactly one undo step.
    /// Returns whether the transaction contained an effective project edit.
    pub fn commit_transaction(&mut self) -> bool {
        let Some(transaction) = self.transaction.take() else {
            return false;
        };
        if !transaction.changed {
            return false;
        }
        self.push_snapshot(transaction.before);
        true
    }

    /// Abandon the open transaction and return its pre-edit state for the
    /// application boundary to restore. History itself is left unchanged.
    pub fn abandon_transaction(&mut self) -> Option<(ProjectSnapshot, bool)> {
        self.last_gesture = None;
        self.transaction
            .take()
            .map(|transaction| (transaction.before, transaction.dirty_before))
    }

    pub fn transaction_active(&self) -> bool {
        self.transaction.is_some()
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.last_gesture = None;
        self.transaction = None;
    }
}
