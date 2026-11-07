//! Arrangement domain unit tests.

use std::sync::Arc;

use super::slicing::MAX_TIMELINE_SLICES;
use super::test_support::*;
use super::*;
use crate::domains::test_support::RecordingEngine;
use crate::state::{AudioClipInspectorField, UiClip};
use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
use vibez_core::midi::{InstrumentKind, MidiNote};
use vibez_core::track::{AudioInputRoute, ClipPlaybackDirection, MediaSourceRef};
use vibez_core::transient::{TransientMarker, TransientMarkerKind};
use vibez_core::warp_marker::WarpMarker;

/// Joins the selected Audio Clips through the background round trip, with
/// Project Media staging replaced by a fixed source.
fn join_selected_audio(
    a: &mut ArrangementFixture,
    engine: &mut RecordingEngine,
) -> (ArrangementAction, ArrangementAction) {
    let mut requested = a.update(
        ArrangementMsg::JoinSelectedClips,
        engine,
        ArrangementCtx::default(),
    );
    let request = requested.join_audio.take().expect("an audio join request");
    let prepared = PreparedAudioJoin {
        joined: render_joined_audio(&request.clips),
        source: joined_source(),
    };
    let applied = a.update(
        ArrangementMsg::ApplyAudioJoin { request, prepared },
        engine,
        ArrangementCtx::default(),
    );
    (requested, applied)
}

fn joined_source() -> MediaSourceRef {
    MediaSourceRef::LocalFile {
        path: "Joined.wav".into(),
    }
}

fn add_audio_clip(
    a: &mut ArrangementFixture,
    track_idx: usize,
    position: u64,
    duration: u64,
) -> (TrackId, ClipId) {
    let audio = Arc::new(vibez_core::audio_buffer::DecodedAudio {
        channels: vec![vec![0.0; (position + duration) as usize]],
        sample_rate: 44100,
    });
    let id = ClipId::new();
    let tid = a.tracks[track_idx].id;
    let clip = UiClip {
        id,
        name: "Clip".to_string(),
        audio,
        source: crate::state::UiClip::test_source(),
        position,
        source_offset: 0,
        start_marker: 0,
        duration,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 0,
        gain_db: Default::default(),
        fades: Default::default(),
        playback_direction: Default::default(),
        transient_markers: Default::default(),
        warp_markers: Default::default(),
        transpose: Default::default(),
        original_bpm: None,
        warped: false,
        warped_to_bpm: None,
        original_audio: None,
    };
    a.tracks[track_idx].clips.push(clip.clone());
    Arc::make_mut(&mut a.arrangement.timeline)
        .ensure(tid)
        .clips
        .push(clip);
    (tid, id)
}

mod audio_inspector;
mod audio_markers;
mod channel_selection;
mod clipboard_loops;
mod content_swap;
mod crossfades;
mod drag_preview;
mod duplicate_block;
mod edge_trims;
mod group_move;
mod loop_off;
mod loop_region;
mod note_clips;
mod overwrite;
mod overwrite_edits;
mod project_tracks;
mod selection;
mod shortest_length;
mod trim_join;
mod unchanged_drags;
