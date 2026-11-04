//! Dragging a Clip's edges changes its length while its content stays at
//! the same Arrangement time, and never past the content it plays.

use super::*;

const CTX: ArrangementCtx = ArrangementCtx {
    samples_per_beat: 100.0,
    playhead_samples: 0,
    playhead_beats: 0.0,
};

/// An Audio Clip at `position` playing `duration` frames of a
/// `sample_frames` long sample from `source_offset`.
fn sample_clip(
    a: &mut ArrangementFixture,
    sample_frames: usize,
    position: u64,
    source_offset: u64,
    duration: u64,
) -> ArrangementSelection {
    let (track_id, clip_id) = add_audio_clip(a, 0, position, duration);
    let clip = &mut a.tracks[0].clips[0];
    clip.audio = Arc::new(vibez_core::audio_buffer::DecodedAudio {
        channels: vec![vec![0.0; sample_frames]],
        sample_rate: 44100,
    });
    clip.source_offset = source_offset;
    clip.start_marker = source_offset;
    ArrangementSelection::AudioClip { track_id, clip_id }
}

fn resize(a: &mut ArrangementFixture, anchor: ArrangementSelection, beats: f64) {
    a.update(
        ArrangementMsg::ResizeSelectedClips {
            anchor,
            new_duration_beats: beats,
        },
        &mut RecordingEngine::default(),
        CTX,
    );
}

#[test]
fn an_unlooped_audio_clip_resized_past_its_sample_stops_at_the_sample_end() {
    let mut a = arrangement_with_tracks(1);
    let anchor = sample_clip(&mut a, 1_000, 0, 200, 500);

    resize(&mut a, anchor, 12.0);

    assert_eq!(a.tracks[0].clips[0].duration, 800);
}

#[test]
fn a_reversed_audio_clip_resized_past_its_sample_stops_at_the_sample_end() {
    let mut a = arrangement_with_tracks(1);
    let anchor = sample_clip(&mut a, 1_000, 0, 200, 500);
    a.tracks[0].clips[0].playback_direction = ClipPlaybackDirection::Reverse;

    resize(&mut a, anchor, 12.0);

    assert_eq!(a.tracks[0].clips[0].duration, 800);
}

#[test]
fn a_warped_audio_clip_resized_past_its_sample_stops_at_the_sample_end() {
    let mut a = arrangement_with_tracks(1);
    let anchor = sample_clip(&mut a, 1_000, 0, 0, 500);
    assert!(a.tracks[0].clips[0].warp_markers.add(250, 400, 0, 500, 500));

    resize(&mut a, anchor, 12.0);

    let clip = &a.tracks[0].clips[0];
    assert_eq!(clip.duration, 1_000);
    assert_eq!(clip.source_end(), 1_000);
}
