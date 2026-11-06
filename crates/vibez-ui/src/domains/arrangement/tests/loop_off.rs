//! Turning a looped Audio Clip's loop off keeps it playing from its start
//! marker, as far as its sample goes.

use super::*;

/// A looped Audio Clip `duration` frames long over a 1000-frame sample,
/// playing the loop over its first 400 frames from `start_marker`.
fn looped_clip(a: &mut ArrangementFixture, duration: u64, start_marker: u64) -> (TrackId, ClipId) {
    let (track_id, clip_id) = add_audio_clip(a, 0, 0, duration);
    let clip = &mut a.tracks[0].clips[0];
    clip.audio = Arc::new(vibez_core::audio_buffer::DecodedAudio {
        channels: vec![vec![0.0; 1_000]],
        sample_rate: 44100,
    });
    clip.loop_enabled = true;
    clip.start_marker = start_marker;
    clip.loop_start = 0;
    clip.loop_end = 400;
    (track_id, clip_id)
}

fn loop_off(a: &mut ArrangementFixture, track_id: TrackId, clip_id: ClipId) -> RecordingEngine {
    let mut engine = RecordingEngine::default();
    a.update(
        ArrangementMsg::ToggleClipLoop(track_id, clip_id),
        &mut engine,
        ArrangementCtx::default(),
    );
    engine
}

/// The source frame `clip` plays at each of its frames, `None` where it is
/// silent because its window has ended.
fn played(clip: &UiClip) -> Vec<Option<u64>> {
    let window_end = clip.source_offset + clip.duration;
    (0..clip.duration)
        .map(|frame| Some(clip.source_frame_at(frame)).filter(|source| *source < window_end))
        .collect()
}

#[test]
fn a_clip_whose_start_marker_lies_past_its_window_plays_on_from_it_when_its_loop_turns_off() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, clip_id) = looped_clip(&mut a, 200, 300);

    let engine = loop_off(&mut a, track_id, clip_id);

    let clip = &a.tracks[0].clips[0];
    assert!(!clip.loop_enabled);
    assert_eq!(clip.duration, 200);
    assert_eq!(played(clip), (300..500).map(Some).collect::<Vec<_>>());
    assert!(engine.0.iter().any(|command| matches!(
        command,
        EngineCommand::SetClipBounds {
            source_offset: 300,
            start_marker: 300,
            duration: 200,
            ..
        }
    )));
}

#[test]
fn a_clip_whose_loop_turns_off_stops_at_the_end_of_its_sample() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, clip_id) = looped_clip(&mut a, 900, 300);

    loop_off(&mut a, track_id, clip_id);

    let clip = &a.tracks[0].clips[0];
    assert_eq!(clip.duration, 700);
    assert_eq!(played(clip), (300..1_000).map(Some).collect::<Vec<_>>());
}

#[test]
fn a_warped_clip_whose_loop_turns_off_plays_its_first_pass_in_place() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, clip_id) = looped_clip(&mut a, 400, 300);
    assert!(a.tracks[0].clips[0]
        .warp_markers
        .add(500, 200, 0, 1_000, 400));
    let before = a.tracks[0].clips[0].clone();

    loop_off(&mut a, track_id, clip_id);

    let after = &a.tracks[0].clips[0];
    for frame in 0..100 {
        let (was, is) = (before.source_frame_at(frame), after.source_frame_at(frame));
        assert!(
            was.abs_diff(is) <= 1,
            "frame {frame} played {was}, now {is}"
        );
    }
}

#[test]
fn a_clip_playing_from_its_window_start_only_loses_its_loop() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, clip_id) = looped_clip(&mut a, 400, 0);
    a.tracks[0].clips[0].fades = vibez_core::track::ClipFades::new(50, 50, 400);
    let before = a.tracks[0].clips[0].clone();

    let engine = loop_off(&mut a, track_id, clip_id);

    let after = &a.tracks[0].clips[0];
    assert_eq!(
        UiClip {
            loop_enabled: true,
            ..after.clone()
        },
        before
    );
    assert!(matches!(
        engine.0.as_slice(),
        [EngineCommand::SetClipLoop { enabled: false, .. }]
    ));
}
