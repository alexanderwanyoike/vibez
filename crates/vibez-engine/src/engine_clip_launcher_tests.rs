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

#[test]
fn clip_record_count_in_and_stop_split_callbacks_without_stopping_other_tracks() {
    let (mut engine, mut commands, mut events, a, b) = setup();
    engine.process(&mut [0.0; 7], 1);
    let prepared = clip(a, 1, &[0.1; 16], true);
    let id = prepared.clip_id.unwrap();
    commands
        .push(EngineCommand::ArmClipRecord {
            free_length: false,
            prepared,
            count_in_bars: 1,
        })
        .unwrap();
    engine.process(&mut [0.0; 19], 1);
    assert_eq!(engine.tracks[0].active_clip.unwrap().position, 3);
    let armed = std::iter::from_fn(|| events.pop().ok()).collect::<Vec<_>>();
    assert!(armed.iter().any(|e| matches!(e, EngineEvent::ClipRecordArmed { clip_id, start: 16, output_start: 23, .. } if *clip_id == id)));
    assert!(armed
        .iter()
        .any(|e| matches!(e, EngineEvent::ClipRecordStarted { at: 16, .. })));
    launch(
        &mut commands,
        vec![clip(b, 2, &[0.2; 5], true)],
        MusicalBoundary::Immediate,
    );
    commands
        .push(EngineCommand::StopClipRecord { immediate: false })
        .unwrap();
    engine.process(&mut [0.0; 15], 1);
    assert!(engine.transport.is_playing());
    assert!(engine.clip_record.is_none());
    assert_eq!(engine.tracks[1].active_clip.unwrap().position, 5);
    assert!(std::iter::from_fn(|| events.pop().ok()).any(|e| matches!(
        e,
        EngineEvent::ClipRecordStopped {
            at: 32,
            started: true,
            ..
        }
    )));
}

#[test]
fn clip_record_cancel_during_count_in_never_starts_the_clip() {
    let (mut engine, mut commands, mut events, a, _) = setup();
    commands
        .push(EngineCommand::ArmClipRecord {
            free_length: false,
            prepared: clip(a, 1, &[0.1; 16], true),
            count_in_bars: 1,
        })
        .unwrap();
    engine.process(&mut [0.0; 3], 1);
    commands
        .push(EngineCommand::StopClipRecord { immediate: false })
        .unwrap();
    engine.process(&mut [0.0; 20], 1);
    assert!(engine.tracks[0].active_clip.is_none());
    let events = std::iter::from_fn(|| events.pop().ok()).collect::<Vec<_>>();
    assert!(events.iter().any(|e| matches!(
        e,
        EngineEvent::ClipRecordStopped {
            at: 3,
            started: false,
            ..
        }
    )));
    assert!(!events
        .iter()
        .any(|e| matches!(e, EngineEvent::ClipRecordStarted { .. })));
}

#[test]
fn live_loop_mode_change_keeps_phase_and_one_shot_stops_at_clip_end() {
    let (mut engine, mut commands, mut events, a, _) = setup();
    let first = clip(a, 1, &[0.1, 0.2, 0.3, 0.4], true);
    let id = first.clip_id;
    launch(&mut commands, vec![first], MusicalBoundary::Immediate);
    engine.process(&mut [0.0; 2], 1);
    let mut replacement = clip(a, 2, &[0.1, 0.2, 0.3, 0.4], false);
    replacement.clip_id = id;
    commands
        .push(EngineCommand::RefreshClip(replacement))
        .unwrap();
    let mut output = [0.0; 4];
    engine.process(&mut output, 1);
    assert_eq!(output, [0.3, 0.4, 0.0, 0.0]);
    assert!(engine.tracks[0].active_clip.is_none());
    assert!(std::iter::from_fn(|| events.pop().ok()).any(|e| matches!(
        e,
        EngineEvent::ClipTransitioned {
            clip_id: None,
            effective_at_samples: 4,
            ..
        }
    )));
}

#[test]
fn transport_stop_finishes_clip_take_on_its_actual_sample() {
    let (mut engine, mut commands, mut events, a, _) = setup();
    commands
        .push(EngineCommand::ArmClipRecord {
            free_length: false,
            prepared: clip(a, 1, &[0.1; 16], true),
            count_in_bars: 0,
        })
        .unwrap();
    engine.process(&mut [0.0; 7], 1);
    commands.push(EngineCommand::Stop).unwrap();
    engine.process(&mut [0.0; 4], 1);
    assert!(!engine.transport.is_playing());
    assert!(engine.clip_record.is_none());
    assert!(std::iter::from_fn(|| events.pop().ok()).any(|e| matches!(
        e,
        EngineEvent::ClipRecordStopped {
            at: 7,
            started: true,
            ..
        }
    )));
}

#[test]
fn free_recording_begins_its_first_loop_on_the_stop_boundary() {
    let (mut engine, mut commands, mut events, a, _) = setup();
    let mut source = clip(a, 1, &[0.1; 32], false);
    let id = source.clip_id;
    source.length_samples = u64::MAX / 4;
    commands
        .push(EngineCommand::ArmClipRecord {
            prepared: source,
            count_in_bars: 0,
            free_length: true,
        })
        .unwrap();
    engine.process(&mut [0.0; 14], 1);
    let mut refreshed = clip(a, 2, &[0.2; 16], true);
    refreshed.clip_id = id;
    commands
        .push(EngineCommand::RefreshClip(refreshed))
        .unwrap();
    commands
        .push(EngineCommand::StopClipRecord { immediate: false })
        .unwrap();
    let mut output = [0.0; 5];
    engine.process(&mut output, 1);
    assert_eq!(output, [0.2; 5]);
    let active = engine.tracks[0].active_clip.unwrap();
    assert_eq!(active.length, 16);
    assert_eq!(active.position, 3);
    assert!(active.looping);
    assert!(std::iter::from_fn(|| events.pop().ok()).any(|e| matches!(
        e,
        EngineEvent::ClipRecordStopped {
            at: 16,
            started: true,
            ..
        }
    )));
}

#[test]
fn clip_segments_keep_live_input_and_resample_capture_aligned_across_wraps() {
    let (mut engine, mut commands, _, a, b) = setup();
    launch(
        &mut commands,
        vec![clip(a, 1, &[0.1, 0.2, 0.3], true)],
        MusicalBoundary::Immediate,
    );
    let input = [0.01, 0.02, 0.03, 0.04, 0.05, 0.06, 0.07];
    let mut output = [0.0; 7];
    let mut capture = [0.0; 7];
    engine.process_block(
        AudioProcessBlock::new(&mut output, 1)
            .with_live_input(b.raw(), &input)
            .with_track_output_capture(a.raw(), &mut capture),
    );
    assert_eq!(capture, [0.1, 0.2, 0.3, 0.1, 0.2, 0.3, 0.1]);
    for ((out, source), live) in output.iter().zip(capture).zip(input) {
        assert!((out - source - live).abs() < 1e-6);
    }
}

#[test]
fn live_edit_updates_active_and_queued_sources_without_moving_their_clocks() {
    let (mut engine, mut commands, mut events, a, b) = setup();
    let original = clip(a, 1, &[0.1; 32], true);
    let id = original.clip_id;
    launch(
        &mut commands,
        vec![original, clip(b, 2, &[0.01, 0.02, 0.03, 0.04], true)],
        MusicalBoundary::Immediate,
    );
    engine.process(&mut [0.0; 3], 1);
    let mut relaunch = clip(a, 3, &[0.1; 32], true);
    relaunch.clip_id = id;
    launch(&mut commands, vec![relaunch], MusicalBoundary::OneBar);
    let mut active = clip(a, 4, &[0.2; 32], true);
    active.clip_id = id;
    let mut queued = clip(a, 5, &[0.2; 32], true);
    queued.clip_id = id;
    commands
        .push(EngineCommand::EditClip { active, queued })
        .unwrap();
    let mut output = [0.0; 14];
    engine.process(&mut output, 1);
    for (index, sample) in output.iter().enumerate() {
        let expected = 0.2 + [0.01, 0.02, 0.03, 0.04][(index + 3) % 4];
        assert!((sample - expected).abs() < 1e-6, "frame {index}: {sample}");
    }
    assert_eq!(engine.tracks[0].active_clip.unwrap().position, 1);
    let events: Vec<_> = std::iter::from_fn(|| events.pop().ok()).collect();
    assert!(events.iter().any(|event| matches!(
        event,
        EngineEvent::ClipSourceRefreshed {
            request_id: 4,
            position: 3,
            effective_at_samples: 3,
            ..
        }
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        EngineEvent::ClipTransitioned {
            request_id: 5,
            effective_at_samples: 16,
            ..
        }
    )));
}

#[test]
fn shortening_a_playing_one_shot_past_its_position_finishes_it() {
    let (mut engine, mut commands, _events, a, _) = setup();
    let original = clip(a, 1, &[0.1; 16], false);
    let id = original.clip_id;
    launch(&mut commands, vec![original], MusicalBoundary::Immediate);
    engine.process(&mut [0.0; 6], 1);
    let mut active = clip(a, 2, &[0.2; 4], false);
    active.clip_id = id;
    let mut queued = clip(a, 3, &[0.2; 4], false);
    queued.clip_id = id;
    commands
        .push(EngineCommand::EditClip { active, queued })
        .unwrap();
    let mut output = [0.0; 4];
    engine.process(&mut output, 1);
    assert_eq!(output, [0.0; 4]);
    assert!(engine.tracks[0].active_clip.is_none());
}

#[test]
fn arrange_play_restores_end_of_content_after_clip_performance() {
    let (mut engine, mut commands, _events, a, _) = setup();
    engine.arrangement_audio_length = Some(8);
    launch(
        &mut commands,
        vec![clip(a, 1, &[0.1; 16], true)],
        MusicalBoundary::OneBar,
    );
    engine.process(&mut [0.0; 4], 1);
    assert_eq!(engine.transport.audio_length(), None);
    commands.push(EngineCommand::Play).unwrap();
    engine.process(&mut [0.0; 10], 1);
    assert_eq!(engine.transport.audio_length(), Some(8));
    assert!(!engine.transport.is_playing());
}

#[test]
fn finishing_a_free_take_does_not_resize_a_replacement_clip() {
    let (mut engine, mut commands, _events, track, _) = setup();
    commands
        .push(EngineCommand::ArmClipRecord {
            free_length: true,
            prepared: clip(track, 1, &[0.1; 16], false),
            count_in_bars: 0,
        })
        .unwrap();
    engine.process(&mut [0.0; 4], 1);
    let replacement = clip(track, 2, &[0.2; 32], false);
    let id = replacement.clip_id.unwrap();
    launch(&mut commands, vec![replacement], MusicalBoundary::Immediate);
    engine.process(&mut [0.0; 1], 1);
    commands
        .push(EngineCommand::StopClipRecord { immediate: true })
        .unwrap();
    engine.process(&mut [], 1);
    let active = engine.tracks[0].active_clip.unwrap();
    assert_eq!(active.clip_id, id);
    assert_eq!(active.length, 32);
    assert_eq!(active.position, 1);
    assert!(!active.looping);
}
