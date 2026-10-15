//! Off-callback storage exchanges retain complete owners for one pending clip or note value.

use std::sync::{
    atomic::{fence, Ordering},
    Arc,
};

use rtrb::{Consumer, Producer, RingBuffer};
use vibez_core::id::{ClipId, TrackId};

use crate::playback_source::{
    EngineClip, EngineNoteClip, NoteInsertionStorage, PreparedPlaybackSource,
};
use vibez_core::midi::MidiNote;
use vibez_core::perform::GrooveGrid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipInsertionTarget {
    Clip(ClipId),
}

#[derive(Clone)]
pub struct ClipInsertionId(Arc<()>);

impl PartialEq for ClipInsertionId {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for ClipInsertionId {}

impl std::fmt::Debug for ClipInsertionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ClipInsertionId")
            .field(&Arc::as_ptr(&self.0))
            .finish()
    }
}

struct ClipStorage<T> {
    live: Vec<T>,
    removed: Vec<T>,
}

impl<T> ClipStorage<T> {
    fn new(capacity: usize) -> Self {
        Self {
            live: Vec::with_capacity(capacity),
            removed: Vec::with_capacity(capacity),
        }
    }

    fn install_if_needed(&mut self, live: &mut Vec<T>, removed: &mut Vec<T>) -> bool {
        let required = live.len().checked_add(1).expect("clip length overflow");
        if live.capacity() >= required && removed.capacity() >= required {
            return true;
        }
        if self.live.capacity() < required || self.removed.capacity() < required {
            return false;
        }
        // Only current engine values move. Empty old allocations stay inside
        // the complete return owner, including the removal staging allocation.
        assert!(self.live.is_empty());
        assert!(self.removed.is_empty());
        assert!(removed.is_empty());
        self.live.append(live);
        std::mem::swap(live, &mut self.live);
        std::mem::swap(removed, &mut self.removed);
        true
    }
}

#[derive(Clone, Copy, Debug)]
enum ClipInsertionKind {
    Audio,
    NoteClip,
    NoteValue,
}

enum ClipInsertionStorage {
    Audio(ClipStorage<EngineClip>),
    NoteClip(ClipStorage<EngineNoteClip>),
    NoteValue(NoteInsertionStorage),
}

impl ClipInsertionStorage {
    fn new(kind: ClipInsertionKind, capacity: usize) -> Self {
        match kind {
            ClipInsertionKind::Audio => Self::Audio(ClipStorage::new(capacity)),
            ClipInsertionKind::NoteClip => Self::NoteClip(ClipStorage::new(capacity)),
            ClipInsertionKind::NoteValue => Self::NoteValue(NoteInsertionStorage::new(capacity)),
        }
    }

    fn kind(&self) -> ClipInsertionKind {
        match self {
            Self::Audio(_) => ClipInsertionKind::Audio,
            Self::NoteClip(_) => ClipInsertionKind::NoteClip,
            Self::NoteValue(_) => ClipInsertionKind::NoteValue,
        }
    }
}

pub(crate) enum ClipInsertionDelta {
    Audio(EngineClip),
    NoteClip(EngineNoteClip),
    NoteValue(MidiNote),
}

/// Required sender-owned control. Construct only outside the audio callback.
///
/// Capacity is a hint, never a content limit or a snapshot of clip values.
/// Native consumers must fulfill every ClipInsertionStorageRequested event.
/// A pending exchange pauses later FIFO commands, including Stop and NoteOff;
/// serving its request is required for progress even when its target is stale.
#[must_use = "send the prepared control with AddClip or drop it off callback"]
pub struct PreparedClipInsertion {
    pub(crate) id: ClipInsertionId,
    sender: Option<Producer<ClipInsertionStorage>>,
    receiver: Consumer<ClipInsertionStorage>,
    storage: ClipInsertionStorage,
    reply: Option<ClipInsertionStorage>,
    pub(crate) clip: Option<ClipInsertionDelta>,
    pub(crate) target: Option<(TrackId, ClipInsertionTarget)>,
}

impl PreparedClipInsertion {
    pub fn new() -> Box<Self> {
        Self::with_capacity(1)
    }

    pub fn with_capacity(capacity: usize) -> Box<Self> {
        Self::prepare(ClipInsertionKind::Audio, capacity, None)
    }

    fn prepare(
        kind: ClipInsertionKind,
        capacity: usize,
        clip: Option<ClipInsertionDelta>,
    ) -> Box<Self> {
        let (sender, receiver) = RingBuffer::new(1);
        Box::new(Self {
            id: ClipInsertionId(Arc::new(())),
            sender: Some(sender),
            receiver,
            storage: ClipInsertionStorage::new(kind, capacity),
            reply: None,
            clip,
            target: None,
        })
    }

    pub fn id(&self) -> ClipInsertionId {
        self.id.clone()
    }

    pub(crate) fn try_apply(
        &mut self,
        source: &mut PreparedPlaybackSource,
    ) -> ClipInsertionAttempt {
        let (_, ClipInsertionTarget::Clip(clip_id)) = self.target.expect("the command supplies its target");
        let storage = self.reply.as_mut().unwrap_or(&mut self.storage);
        let (ready, length) = match (
            storage,
            self.clip.as_ref().expect("one original insertion delta"),
        ) {
            (ClipInsertionStorage::Audio(storage), ClipInsertionDelta::Audio(_)) => (
                storage.install_if_needed(&mut source.clips, &mut source.removed_audio_clips),
                source.clips.len(),
            ),
            (ClipInsertionStorage::NoteClip(storage), ClipInsertionDelta::NoteClip(_)) => (
                storage.install_if_needed(&mut source.note_clips, &mut source.removed_note_clips),
                source.note_clips.len(),
            ),
            (ClipInsertionStorage::NoteValue(storage), ClipInsertionDelta::NoteValue(_)) => {
                let Some(clip) = source.note_clips.iter_mut().find(|clip| clip.id == clip_id)
                else {
                    return ClipInsertionAttempt::MissingClip;
                };
                (storage.install_if_needed(clip), clip.notes.len())
            }
            _ => unreachable!("private preparation keeps delta and storage kinds paired"),
        };
        if !ready {
            return ClipInsertionAttempt::NeedsCapacity(
                length.checked_add(1).expect("insertion length overflow"),
            );
        }
        match self.clip.take().expect("one original insertion delta") {
            ClipInsertionDelta::Audio(clip) => source.clips.push(clip),
            ClipInsertionDelta::NoteClip(clip) => source.note_clips.push(clip),
            ClipInsertionDelta::NoteValue(note) => {
                source
                    .note_clips
                    .iter_mut()
                    .find(|clip| clip.id == clip_id)
                    .expect("the admitted first note clip is still current")
                    .push_note(note);
            }
        }
        ClipInsertionAttempt::Applied
    }

    pub(crate) fn affects_length(&self) -> bool {
        !matches!(self.storage, ClipInsertionStorage::NoteValue(_))
    }

    pub(crate) fn request(&mut self, required: usize) -> ClipInsertionStorageRequest {
        let (track_id, target) = self.target.expect("the command supplies its target");
        ClipInsertionStorageRequest {
            id: self.id.clone(),
            track_id,
            target,
            capacity: required.checked_mul(2).unwrap_or(required),
            kind: self.storage.kind(),
            sender: self
                .sender
                .take()
                .expect("one storage request per insertion"),
        }
    }

    pub(crate) fn receive(&mut self) -> ClipInsertionReply {
        if let Ok(reply) = self.receiver.pop() {
            self.reply = Some(reply);
            return ClipInsertionReply::Ready;
        }
        if !self.receiver.is_abandoned() {
            return ClipInsertionReply::Waiting;
        }
        // rtrb permits a final push followed by producer destruction. Its
        // documented Acquire fence/recheck closes the preceding empty race.
        fence(Ordering::Acquire);
        if let Ok(reply) = self.receiver.pop() {
            self.reply = Some(reply);
            ClipInsertionReply::Ready
        } else {
            ClipInsertionReply::Abandoned
        }
    }
}

/// An empty note clip and its complete storage exchange, prepared off callback.
///
/// The private wrapper cannot be used as an audio control or carry prepopulated
/// notes. Later AddNote commands retain their existing separate FIFO behavior.
#[must_use = "send this prepared note clip or drop it off callback"]
pub struct PreparedNoteClipInsertion(Box<PreparedClipInsertion>);

impl PreparedNoteClipInsertion {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        clip_id: ClipId,
        position_beats: f64,
        duration_beats: f64,
        start_marker_beats: f64,
        loop_enabled: bool,
        loop_start_beats: f64,
        loop_end_beats: f64,
        groove_grid: GrooveGrid,
    ) -> Self {
        Self::with_capacity(
            clip_id,
            position_beats,
            duration_beats,
            start_marker_beats,
            loop_enabled,
            loop_start_beats,
            loop_end_beats,
            groove_grid,
            1,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_capacity(
        clip_id: ClipId,
        position_beats: f64,
        duration_beats: f64,
        start_marker_beats: f64,
        loop_enabled: bool,
        loop_start_beats: f64,
        loop_end_beats: f64,
        groove_grid: GrooveGrid,
        capacity: usize,
    ) -> Self {
        let clip = EngineNoteClip::new(
            clip_id,
            position_beats,
            duration_beats,
            Vec::new(),
            start_marker_beats,
            loop_enabled,
            loop_start_beats,
            loop_end_beats,
            groove_grid,
        );
        Self(PreparedClipInsertion::prepare(
            ClipInsertionKind::NoteClip,
            capacity,
            Some(ClipInsertionDelta::NoteClip(clip)),
        ))
    }

    pub fn id(&self) -> ClipInsertionId {
        self.0.id()
    }

    pub(crate) fn into_control(self) -> (ClipId, Box<PreparedClipInsertion>) {
        let Some(ClipInsertionDelta::NoteClip(clip)) = self.0.clip.as_ref() else {
            unreachable!("the opaque wrapper owns one empty note clip");
        };
        (clip.id, self.0)
    }
}

/// One original note value and its complete storage exchange, prepared off callback.
#[must_use = "send this prepared note or drop it off callback"]
pub struct PreparedNoteInsertion(Box<PreparedClipInsertion>);

impl PreparedNoteInsertion {
    pub fn new(note: MidiNote) -> Self {
        Self::with_capacity(note, 1)
    }

    pub fn with_capacity(note: MidiNote, capacity: usize) -> Self {
        Self(PreparedClipInsertion::prepare(
            ClipInsertionKind::NoteValue,
            capacity,
            Some(ClipInsertionDelta::NoteValue(note)),
        ))
    }

    pub fn id(&self) -> ClipInsertionId {
        self.0.id()
    }

    pub(crate) fn into_control(self) -> Box<PreparedClipInsertion> {
        self.0
    }
}

pub(crate) enum ClipInsertionAttempt {
    Applied,
    NeedsCapacity(usize),
    MissingClip,
}

pub(crate) enum ClipInsertionReply {
    Ready,
    Waiting,
    Abandoned,
}

/// Consume this on the UI/native event-consumer thread, including stale targets.
/// Dropping it unfulfilled makes the pending command fail with Abandoned.
#[must_use = "fulfill off callback or explicitly abandon the original edit"]
pub struct ClipInsertionStorageRequest {
    pub id: ClipInsertionId,
    pub track_id: TrackId,
    pub target: ClipInsertionTarget,
    capacity: usize,
    kind: ClipInsertionKind,
    sender: Producer<ClipInsertionStorage>,
}

impl ClipInsertionStorageRequest {
    /// Allocate synchronously on the UI/native consumer thread and reply through
    /// the independent mailbox. Never call this inside an audio callback or
    /// replace it with a command queued behind the blocked original edit.
    pub fn fulfill(mut self) {
        // A consumer may already be destroyed. Even a successful orphaned push
        // remains owned by this producer and is destroyed here off callback.
        let _ = self
            .sender
            .push(ClipInsertionStorage::new(self.kind, self.capacity));
    }
}

impl std::fmt::Debug for ClipInsertionStorageRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClipInsertionStorageRequest")
            .field("id", &self.id)
            .field("track_id", &self.track_id)
            .field("kind", &self.kind)
            .field("target", &self.target)
            .field("capacity", &self.capacity)
            .finish()
    }
}

impl PartialEq for ClipInsertionStorageRequest {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipInsertionOutcome {
    Applied,
    MissingTrack,
    MissingClip,
    Abandoned,
}

/// Every displaced/rejected owner and the final mailbox endpoint stay alive
/// until this complete payload is destroyed outside the audio callback.
#[must_use = "retain or destroy the complete insertion owner off callback"]
pub struct CompletedClipInsertion {
    pub track_id: TrackId,
    pub target: ClipInsertionTarget,
    pub outcome: ClipInsertionOutcome,
    pub(crate) owner: Box<PreparedClipInsertion>,
}

impl CompletedClipInsertion {
    pub fn id(&self) -> ClipInsertionId {
        self.owner.id()
    }
}

impl std::fmt::Debug for CompletedClipInsertion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompletedClipInsertion")
            .field("id", &self.owner.id)
            .field("track_id", &self.track_id)
            .field("kind", &self.owner.storage.kind())
            .field("target", &self.target)
            .field("outcome", &self.outcome)
            .finish()
    }
}

impl PartialEq for CompletedClipInsertion {
    fn eq(&self, other: &Self) -> bool {
        self.owner.id == other.owner.id && self.outcome == other.outcome
    }
}

#[cfg(test)]
#[path = "note_clip_insertion_owner_tests.rs"]
mod note_owner_tests;

#[cfg(test)]
#[path = "note_value_insertion_owner_tests.rs"]
mod note_value_owner_tests;
