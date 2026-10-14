//! Off-callback storage exchanges and complete owners for one pending AddClip.

use std::sync::{
    atomic::{fence, Ordering},
    Arc,
};

use rtrb::{Consumer, Producer, RingBuffer};
use vibez_core::id::{ClipId, TrackId};

use crate::playback_source::{EngineClip, PreparedPlaybackSource};

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

pub(crate) struct ClipInsertionStorage {
    clips: Vec<EngineClip>,
    removed_audio_clips: Vec<EngineClip>,
}

impl ClipInsertionStorage {
    fn new(capacity: usize) -> Self {
        Self {
            clips: Vec::with_capacity(capacity),
            removed_audio_clips: Vec::with_capacity(capacity),
        }
    }
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
    pub(crate) clip: Option<EngineClip>,
    pub(crate) target: Option<(TrackId, ClipId)>,
}

impl PreparedClipInsertion {
    pub fn new() -> Box<Self> {
        Self::with_capacity(1)
    }

    pub fn with_capacity(capacity: usize) -> Box<Self> {
        let (sender, receiver) = RingBuffer::new(1);
        Box::new(Self {
            id: ClipInsertionId(Arc::new(())),
            sender: Some(sender),
            receiver,
            storage: ClipInsertionStorage::new(capacity),
            reply: None,
            clip: None,
            target: None,
        })
    }

    pub fn id(&self) -> ClipInsertionId {
        self.id.clone()
    }

    pub(crate) fn try_apply(&mut self, source: &mut PreparedPlaybackSource) -> bool {
        let required = source
            .clips
            .len()
            .checked_add(1)
            .expect("clip length overflow");
        if source.clips.capacity() < required || source.removed_audio_clips.capacity() < required {
            let storage = self.reply.as_mut().unwrap_or(&mut self.storage);
            if storage.clips.capacity() < required
                || storage.removed_audio_clips.capacity() < required
            {
                return false;
            }
            // Only live engine values move. The exchanged allocations, even
            // when empty, remain inside the returned control for UI destruction.
            storage.clips.append(&mut source.clips);
            assert!(source.removed_audio_clips.is_empty());
            std::mem::swap(&mut source.clips, &mut storage.clips);
            std::mem::swap(
                &mut source.removed_audio_clips,
                &mut storage.removed_audio_clips,
            );
        }
        source
            .clips
            .push(self.clip.take().expect("one original insertion delta"));
        true
    }

    pub(crate) fn request(&mut self, required: usize) -> ClipInsertionStorageRequest {
        let (track_id, clip_id) = self.target.expect("the command supplies its target");
        ClipInsertionStorageRequest {
            id: self.id.clone(),
            track_id,
            clip_id,
            capacity: required.checked_mul(2).unwrap_or(required),
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
    pub clip_id: ClipId,
    capacity: usize,
    sender: Producer<ClipInsertionStorage>,
}

impl ClipInsertionStorageRequest {
    /// Allocate synchronously on the UI/native consumer thread and reply through
    /// the independent mailbox. Never call this inside an audio callback or
    /// replace it with a command queued behind the blocked original edit.
    pub fn fulfill(mut self) {
        // A consumer may already be destroyed. Even a successful orphaned push
        // remains owned by this producer and is destroyed here off callback.
        let _ = self.sender.push(ClipInsertionStorage::new(self.capacity));
    }
}

impl std::fmt::Debug for ClipInsertionStorageRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClipInsertionStorageRequest")
            .field("id", &self.id)
            .field("track_id", &self.track_id)
            .field("clip_id", &self.clip_id)
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
    Abandoned,
}

/// Every displaced/rejected owner and the final mailbox endpoint stay alive
/// until this complete payload is destroyed outside the audio callback.
#[must_use = "retain or destroy the complete insertion owner off callback"]
pub struct CompletedClipInsertion {
    pub track_id: TrackId,
    pub clip_id: ClipId,
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
            .field("clip_id", &self.clip_id)
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
#[path = "clip_insertion_pressure_tests.rs"]
mod pressure_tests;
