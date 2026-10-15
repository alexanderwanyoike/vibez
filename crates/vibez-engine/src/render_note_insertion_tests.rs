//! Actual offline channel preparation retains note schedules and matching live/removal storage.

use super::*;
use crate::playback_source::note_insertion_tests::{
    resident_clip, verify_prepared_note_storage_growth,
};
use vibez_core::midi::{NoteClipInfo, TrackKind};

#[test]
fn offline_note_channel_prepares_actual_content_and_grows_live_and_staging_at_boundaries() {
    for count in [1, 2, 20, 21, 128, 129, 257] {
        let track = TrackInfo {
            id: TrackId::new(),
            name: "Offline note storage".into(),
            gain: 1.0,
            pan: 0.0,
            mute: false,
            solo: false,
            audio_input_route: Default::default(),
            input_monitoring: Default::default(),
            swing_offset: None,
            effects: Vec::new(),
            kind: TrackKind::Audio,
            color_index: 0,
            instrument: None,
            native_instrument: None,
            plugin_instrument: None,
            automation: Vec::new(),
            sends: Vec::new(),
        };
        let track_id = track.id;
        let request = BounceRequest {
            tracks: vec![track],
            master: None,
            buses: Vec::new(),
            audio_clips: Vec::new(),
            note_clips: (0..count)
                .map(|index| {
                    let clip = resident_clip(index, ClipId::new());
                    NoteClipInfo {
                        id: clip.id,
                        track_id,
                        name: format!("Note {index}"),
                        position_beats: clip.position_beats,
                        duration_beats: clip.duration_beats,
                        notes: clip.notes,
                        start_marker_beats: Some(clip.start_marker_beats),
                        loop_enabled: clip.loop_enabled,
                        loop_start_beats: clip.loop_start_beats,
                        loop_end_beats: clip.loop_end_beats,
                        groove_grid: clip.groove_grid,
                    }
                })
                .collect(),
            clip_audio: HashMap::new(),
            sampler_audio: HashMap::new(),
            drum_pad_audio: HashMap::new(),
            mode: BounceMode::Master,
            range_samples: (0, 512),
            bpm: 120.0,
            sample_rate: 44_100,
            swing: vibez_core::perform::SwingAmount::STRAIGHT,
        };
        let track = &request.tracks[0];
        let mut warnings = Vec::new();
        let channel = super::channel(
            &request,
            track,
            SourceTap::AfterEffects,
            None,
            &mut warnings,
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(channel.playback_source.note_clips.len(), count);
        for (original, prepared) in request
            .note_clips
            .iter()
            .zip(&channel.playback_source.note_clips)
        {
            assert_eq!(original.id, prepared.id);
            assert_eq!(original.notes, prepared.notes);
            assert_ne!(original.notes.as_ptr(), prepared.notes.as_ptr());
        }
        verify_prepared_note_storage_growth(*channel.playback_source, count);
    }
}
