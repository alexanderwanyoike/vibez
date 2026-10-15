//! Offline channel preparation preserves actual inner note/cache content before live growth.

use super::*;
use crate::playback_source::note_value_insertion_tests::construction_tests::{
    boundary_clip, verify_note_value_source_growth,
};

#[test]
fn offline_channel_note_values_cross_full_notes_and_cache_at_seven_boundaries() {
    for count in [1, 2, 20, 21, 128, 129, 257] {
        let track = TrackInfo {
            id: TrackId::new(), name: "Offline inner notes".into(), gain: 1.0, pan: 0.0,
            mute: false, solo: false, audio_input_route: Default::default(), input_monitoring: Default::default(),
            swing_offset: None, effects: Vec::new(), kind: TrackKind::Audio, color_index: 0,
            instrument: None, native_instrument: None, plugin_instrument: None, automation: Vec::new(), sends: Vec::new(),
        };
        let track_id = track.id;
        let target = boundary_clip(count, ClipId::new());
        let mut other = boundary_clip(3, ClipId::new());
        other.position_beats = 128.0;
        other.duration_beats = 8.0;
        other.loop_enabled = true;
        other.loop_start_beats = 0.25;
        other.loop_end_beats = 4.0;
        other.start_marker_beats = 0.25;
        other.groove_grid = vibez_core::perform::GrooveGrid::Eighth;
        let note_clips = [target, other].into_iter().map(|clip| NoteClipInfo {
            id: clip.id, track_id, name: "Offline complete note data".into(), position_beats: clip.position_beats,
            duration_beats: clip.duration_beats, notes: clip.notes, start_marker_beats: Some(clip.start_marker_beats),
            loop_enabled: clip.loop_enabled, loop_start_beats: clip.loop_start_beats,
            loop_end_beats: clip.loop_end_beats, groove_grid: clip.groove_grid,
        }).collect();
        let request = BounceRequest {
            tracks: vec![track], master: None, buses: Vec::new(), audio_clips: Vec::new(), note_clips,
            clip_audio: HashMap::new(), sampler_audio: HashMap::new(), drum_pad_audio: HashMap::new(),
            mode: BounceMode::Master, range_samples: (0, 512), bpm: 120.0, sample_rate: 44_100,
            swing: vibez_core::perform::SwingAmount::new(0.62),
        };
        let mut warnings = Vec::new();
        let channel = super::super::channel(&request, &request.tracks[0], SourceTap::AfterEffects, None, &mut warnings);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(channel.playback_source.note_clips.len(), 2);
        for (original, prepared) in request.note_clips.iter().zip(&channel.playback_source.note_clips) {
            assert_eq!(original.id, prepared.id);
            assert_eq!(original.notes, prepared.notes);
            assert_ne!(original.notes.as_ptr(), prepared.notes.as_ptr());
            assert_eq!(original.position_beats, prepared.position_beats);
            assert_eq!(original.duration_beats, prepared.duration_beats);
            assert_eq!(original.start_marker_beats, Some(prepared.start_marker_beats));
            assert_eq!(original.loop_enabled, prepared.loop_enabled);
            assert_eq!(original.loop_start_beats, prepared.loop_start_beats);
            assert_eq!(original.loop_end_beats, prepared.loop_end_beats);
            assert_eq!(original.groove_grid, prepared.groove_grid);
        }
        verify_note_value_source_growth(*channel.playback_source, count);
        assert_eq!(request.note_clips[0].notes.len(), count);
    }
}
