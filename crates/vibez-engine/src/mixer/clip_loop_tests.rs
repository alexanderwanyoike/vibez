//! A looped clip plays its loop whatever the clip's own length.

use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::MidiNote;
use vibez_core::perform::{GrooveGrid, SwingAmount};

use super::{collect_timed_note_events, EngineClip, EngineNoteClip, EngineTrack};

const SAMPLES_PER_BEAT: f64 = 100.0;

fn note(pitch: u8, start_beat: f64) -> MidiNote {
    MidiNote {
        pitch,
        velocity: 100,
        start_beat,
        duration_beats: 0.5,
    }
}

fn note_ons(clip: EngineNoteClip, frames: usize) -> Vec<(u32, u8)> {
    let mut ons = Vec::new();
    let mut offs = Vec::new();
    collect_timed_note_events(
        &[clip],
        0,
        frames,
        SAMPLES_PER_BEAT,
        SwingAmount::STRAIGHT,
        &mut ons,
        &mut offs,
    );
    ons.into_iter()
        .map(|(frame, pitch, _)| (frame, pitch))
        .collect()
}

#[test]
fn a_midi_clip_shorter_than_its_loop_plays_the_loop_from_its_start_marker() {
    let clip = EngineNoteClip::new(
        ClipId::new(),
        0.0,
        3.0,
        vec![note(60, 0.0), note(62, 3.0)],
        2.0,
        true,
        0.0,
        4.0,
        GrooveGrid::Off,
    );

    assert_eq!(note_ons(clip, 600), [(100, 62), (200, 60)]);
}

#[test]
fn a_midi_clip_longer_than_its_loop_repeats_the_loop() {
    let clip = EngineNoteClip::new(
        ClipId::new(),
        0.0,
        8.0,
        vec![note(60, 0.0), note(62, 3.0)],
        0.0,
        true,
        0.0,
        2.0,
        GrooveGrid::Off,
    );

    assert_eq!(
        note_ons(clip, 1_000),
        [(0, 60), (200, 60), (400, 60), (600, 60)]
    );
}

#[test]
fn an_audio_clip_shorter_than_its_loop_plays_the_loop_from_its_start_marker() {
    let audio = Arc::new(DecodedAudio {
        channels: vec![(0..100).map(|frame| frame as f32).collect()],
        sample_rate: 44_100,
    });
    let mut track = EngineTrack::new(TrackId::new());
    track.playback_source.push_audio_clip(EngineClip {
        id: ClipId::new(),
        audio,
        position: 0,
        source_offset: 0,
        start_marker: 50,
        duration: 40,
        loop_enabled: true,
        loop_start: 0,
        loop_end: 80,
        linear_gain: 1.0,
        fades: Default::default(),
        playback_direction: Default::default(),
        warp_markers: Default::default(),
    });

    assert!(track.render(0, 40, 1, None));
    let played: Vec<f32> = track.mix_buffer[..40].to_vec();
    let expected: Vec<f32> = (50..80).chain(0..10).map(|frame| frame as f32).collect();
    assert_eq!(played, expected);
}
