use super::*;
use crate::playback_source::{EngineClip, PreparedClipPlayback, PreparedPlaybackSource};
use vibez_core::id::ClipId;
use vibez_core::perform::MusicalBoundary;

fn clip(
    track_id: TrackId,
    request_id: u64,
    samples: &[f32],
    looping: bool,
) -> Box<PreparedClipPlayback> {
    let id = ClipId::new();
    Box::new(PreparedClipPlayback {
        track_id,
        clip_id: Some(id),
        request_id,
        length_samples: samples.len() as u64,
        looping,
        source: Box::new(PreparedPlaybackSource::new(
            vec![EngineClip {
                id,
                audio: Arc::new(DecodedAudio {
                    channels: vec![samples.to_vec()],
                    sample_rate: 8,
                }),
                position: 0,
                source_offset: 0,
                start_marker: 0,
                duration: samples.len() as u64,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
                linear_gain: 1.0,
                fades: Default::default(),
                playback_direction: Default::default(),
                warp_markers: Default::default(),
            }],
            vec![],
            vec![],
        )),
    })
}
fn stop(track_id: TrackId) -> Box<PreparedClipPlayback> {
    Box::new(PreparedClipPlayback {
        track_id,
        clip_id: None,
        request_id: 99,
        length_samples: 1,
        looping: false,
        source: Box::default(),
    })
}
fn setup() -> (
    AudioEngine,
    rtrb::Producer<EngineCommand>,
    rtrb::Consumer<EngineEvent>,
    TrackId,
    TrackId,
) {
    let (mut engine, mut commands, events) = AudioEngine::new();
    let a = TrackId::new();
    let b = TrackId::new();
    for command in [
        EngineCommand::SetSampleRate(8),
        EngineCommand::SetBpm(120.0),
        EngineCommand::AddTrack(a, "A".into()),
        EngineCommand::AddTrack(b, "B".into()),
    ] {
        commands.push(command).unwrap();
    }
    engine.process(&mut [], 1);
    (engine, commands, events, a, b)
}
#[allow(clippy::vec_box)]
fn launch(
    commands: &mut rtrb::Producer<EngineCommand>,
    clips: Vec<Box<PreparedClipPlayback>>,
    quantization: MusicalBoundary,
) {
    commands
        .push(EngineCommand::QueueClips {
            clips,
            quantization,
        })
        .unwrap();
}
#[test]
fn replacing_one_track_at_a_mid_buffer_boundary_preserves_the_other_tracks_phase() {
    let (mut engine, mut commands, mut events, a, b) = setup();
    launch(
        &mut commands,
        vec![
            clip(a, 1, &[0.1, 0.2, 0.3], true),
            clip(b, 2, &[0.01; 16], true),
        ],
        MusicalBoundary::OneBar,
    );
    engine.process(&mut [0.0; 14], 1);
    launch(
        &mut commands,
        vec![clip(b, 3, &[0.05; 16], true)],
        MusicalBoundary::OneBar,
    );
    let mut output = [0.0; 5];
    engine.process(&mut output, 1);
    for (got, expected) in output.iter().zip([0.31, 0.11, 0.25, 0.35, 0.15]) {
        assert!((got - expected).abs() < 1e-5, "{output:?}");
    }
    assert_eq!(engine.transport.position(), 0);
    assert!(
        std::iter::from_fn(|| events.pop().ok()).any(|event| matches!(
            event,
            EngineEvent::ClipTransitioned {
                request_id: 3,
                effective_at_samples: 16,
                ..
            }
        ))
    );
}
#[test]
fn row_batch_starts_and_stops_all_targets_at_one_boundary_and_latest_request_wins() {
    let (mut engine, mut commands, mut events, a, b) = setup();
    launch(
        &mut commands,
        vec![clip(a, 1, &[0.1; 32], true), clip(b, 2, &[0.2; 32], true)],
        MusicalBoundary::OneBar,
    );
    engine.process(&mut [0.0; 15], 1);
    launch(
        &mut commands,
        vec![clip(a, 3, &[0.4; 32], true)],
        MusicalBoundary::OneBar,
    );
    launch(
        &mut commands,
        vec![stop(a), clip(b, 4, &[0.05; 32], true)],
        MusicalBoundary::OneBar,
    );
    let mut output = [0.0; 3];
    engine.process(&mut output, 1);
    assert!((output[0] - 0.3).abs() < 1e-5);
    assert!((output[1] - 0.05).abs() < 1e-5);
    assert!((output[2] - 0.05).abs() < 1e-5);
    let transitions: Vec<_> = std::iter::from_fn(|| events.pop().ok())
        .filter_map(|event| {
            if let EngineEvent::ClipTransitioned {
                request_id,
                effective_at_samples,
                ..
            } = event
            {
                Some((request_id, effective_at_samples))
            } else {
                None
            }
        })
        .collect();
    assert!(transitions.contains(&(4, 16)));
    assert!(transitions.contains(&(99, 16)));
    assert!(!transitions.iter().any(|(id, _)| *id == 3));
}
#[test]
fn one_shot_ends_without_stopping_other_clips_or_leaking_arrange_content() {
    let (mut engine, mut commands, _, a, b) = setup();
    engine
        .tracks
        .iter_mut()
        .find(|track| track.id == a)
        .unwrap()
        .playback_source = clip(a, 0, &[0.7; 32], true).source;
    launch(
        &mut commands,
        vec![clip(a, 1, &[0.1; 2], false), clip(b, 2, &[0.02; 3], true)],
        MusicalBoundary::Immediate,
    );
    let mut output = [0.0; 7];
    engine.process(&mut output, 1);
    assert!((output[0] - 0.12).abs() < 1e-5);
    assert!((output[2] - 0.02).abs() < 1e-5);
    assert!((output[6] - 0.02).abs() < 1e-5);
    assert!(engine.transport.is_playing());
    commands.push(EngineCommand::Stop).unwrap();
    engine.process(&mut [0.0; 1], 1);
    assert!(!engine.clip_performance);
    assert!(engine
        .tracks
        .iter()
        .all(|track| track.active_clip.is_none() && track.queued_clip.is_none()));
}
#[test]
fn capture_start_reports_each_exact_local_position_and_stop_cancels_pending_launches() {
    let (mut engine, mut commands, mut events, a, b) = setup();
    launch(
        &mut commands,
        vec![clip(a, 1, &[0.1; 3], true), clip(b, 2, &[0.1; 8], true)],
        MusicalBoundary::Immediate,
    );
    engine.process(&mut [0.0; 6], 1);
    commands
        .push(EngineCommand::StartPerformanceCapture)
        .unwrap();
    engine.process(&mut [0.0; 1], 1);
    let starts: Vec<_> = std::iter::from_fn(|| events.pop().ok())
        .filter_map(|event| {
            if let EngineEvent::ClipCaptureSource {
                track_id,
                position,
                effective_at_samples,
            } = event
            {
                Some((track_id, position, effective_at_samples))
            } else {
                None
            }
        })
        .collect();
    assert!(starts.contains(&(a, 0, 6)));
    assert!(starts.contains(&(b, 6, 6)));
    launch(
        &mut commands,
        vec![clip(a, 3, &[0.3; 8], true)],
        MusicalBoundary::OneBar,
    );
    commands.push(EngineCommand::Stop).unwrap();
    engine.process(&mut [0.0; 20], 1);
    assert!(!std::iter::from_fn(|| events.pop().ok())
        .any(|event| matches!(event, EngineEvent::ClipTransitioned { request_id: 3, .. })));
}

#[test]
fn midi_replacement_flushes_only_its_track_and_preserves_sustained_notes_elsewhere() {
    use crate::playback_source::EngineNoteClip;
    use std::sync::Mutex;
    use vibez_core::midi::{InstrumentKind, MidiNote};
    struct Notes(Arc<Mutex<Vec<(u8, bool)>>>);
    impl vibez_instruments::Instrument for Notes {
        fn instrument_kind(&self) -> InstrumentKind {
            InstrumentKind::SubtractiveSynth
        }
        fn param_descriptors(&self) -> &'static [vibez_core::effect::ParamDescriptor] {
            &[]
        }
        fn set_param(&mut self, _: usize, _: f32) -> bool {
            false
        }
        fn get_param(&self, _: usize) -> f32 {
            0.0
        }
        fn note_on(&mut self, pitch: u8, _: u8) {
            self.0.lock().unwrap().push((pitch, true));
        }
        fn note_off(&mut self, pitch: u8) {
            self.0.lock().unwrap().push((pitch, false));
        }
        fn render(&mut self, _: &mut [f32], _: usize) {}
        fn reset(&mut self) {}
    }
    let (mut engine, mut commands, _, a, b) = setup();
    let log = Arc::new(Mutex::new(Vec::new()));
    for track in [a, b] {
        commands
            .push(EngineCommand::SetPluginInstrument {
                track_id: track,
                instrument: Box::new(Notes(Arc::clone(&log))),
            })
            .unwrap();
    }
    let note = |track, pitch, request_id| {
        let mut prepared = clip(track, request_id, &[0.0; 16], true);
        prepared.source.clips.clear();
        prepared.source.note_clips.push(EngineNoteClip::new(
            prepared.clip_id.unwrap(),
            0.0,
            4.0,
            vec![MidiNote {
                pitch,
                velocity: 100,
                start_beat: 0.0,
                duration_beats: 3.0,
            }],
            0.0,
            false,
            0.0,
            0.0,
            Default::default(),
        ));
        prepared
    };
    launch(
        &mut commands,
        vec![note(a, 36, 1), note(b, 60, 2)],
        MusicalBoundary::Immediate,
    );
    engine.process(&mut [0.0; 3], 1);
    launch(
        &mut commands,
        vec![note(b, 64, 3)],
        MusicalBoundary::OneBeat,
    );
    engine.process(&mut [0.0; 4], 1);
    let heard = log.lock().unwrap();
    assert_eq!(
        heard
            .iter()
            .filter(|(pitch, on)| *pitch == 36 && *on)
            .count(),
        1
    );
    assert!(!heard.contains(&(36, false)));
    assert!(heard.contains(&(60, false)));
    assert!(heard.contains(&(64, true)));
}
