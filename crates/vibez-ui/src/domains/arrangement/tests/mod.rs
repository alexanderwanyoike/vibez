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
        source: None,
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
mod clipboard_loops;
mod crossfades;
mod note_clips;
mod project_tracks;
mod selection;
mod trim_join;
