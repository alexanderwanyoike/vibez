//! Section playback regression tests. These exercise the public command/event
//! boundary so the source-switch implementation remains replaceable.

use super::*;
use graph_test_support::{graph_engine, routing_channel, routing_effect, routing_plan};
use vibez_core::routing::RoutingChannel;

use std::sync::{Arc, Mutex};

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::effect::EffectType;
use vibez_core::id::{ClipId, EffectId, SectionId, TrackId};
use vibez_core::midi::{InstrumentKind, MidiNote};

use crate::playback_source::{
    EngineClip, EngineNoteClip, PreparedPlaybackSource, PreparedSectionPlaybackSource,
};

struct NoteLogInstrument(Arc<Mutex<Vec<(bool, u8)>>>);

struct TailEffect {
    memory: f32,
}

impl vibez_dsp::effect::AudioEffect for TailEffect {
    fn effect_type(&self) -> EffectType {
        EffectType::Gain
    }

    fn param_descriptors(&self) -> &'static [vibez_core::effect::ParamDescriptor] {
        &[]
    }

    fn set_param(&mut self, _index: usize, _value: f32) -> bool {
        false
    }

    fn get_param(&self, _index: usize) -> f32 {
        0.0
    }

    fn process(&mut self, buffer: &mut [f32], _channels: usize) {
        for sample in buffer {
            if *sample != 0.0 {
                self.memory = *sample;
            } else {
                self.memory *= 0.5;
                *sample = self.memory;
            }
        }
    }

    fn reset(&mut self) {
        self.memory = 0.0;
    }
}

impl vibez_instruments::Instrument for NoteLogInstrument {
    fn instrument_kind(&self) -> InstrumentKind {
        InstrumentKind::SubtractiveSynth
    }

    fn param_descriptors(&self) -> &'static [vibez_core::effect::ParamDescriptor] {
        &[]
    }

    fn set_param(&mut self, _index: usize, _value: f32) -> bool {
        false
    }

    fn get_param(&self, _index: usize) -> f32 {
        0.0
    }

    fn note_on(&mut self, pitch: u8, _velocity: u8) {
        self.0.lock().unwrap().push((true, pitch));
    }

    fn note_off(&mut self, pitch: u8) {
        self.0.lock().unwrap().push((false, pitch));
    }

    fn render(&mut self, _buffer: &mut [f32], _channels: usize) {}

    fn reset(&mut self) {}
}

fn constant_audio(frames: usize, value: f32) -> Arc<DecodedAudio> {
    Arc::new(DecodedAudio {
        channels: vec![vec![value; frames], vec![value; frames]],
        sample_rate: 44_100,
    })
}

#[test]
fn resident_section_switches_immediately_and_returns_the_displaced_source() {
    let track_id = TrackId::new();
    let (mut engine, mut commands, mut events) =
        graph_engine(&[routing_channel(track_id), routing_channel(TrackId::MASTER)]);
    let section_id = SectionId::new();
    commands.push(EngineCommand::add_track(track_id)).unwrap();
    commands
        .push(EngineCommand::LaunchSection(Box::new(
            PreparedSectionPlaybackSource::new(
                section_id,
                4.0,
                true,
                vec![(
                    track_id,
                    PreparedPlaybackSource::new(
                        vec![EngineClip {
                            id: ClipId::new(),
                            audio: constant_audio(16, 0.25),
                            position: 0,
                            source_offset: 0,
                            start_marker: 0,
                            duration: 16,
                            loop_enabled: false,
                            loop_start: 0,
                            loop_end: 0,
                            linear_gain: 1.0,
                            fades: Default::default(),
                            playback_direction: Default::default(),
                            warp_markers: Default::default(),
                        }],
                        Vec::new(),
                        Vec::new(),
                    ),
                )],
            ),
        )))
        .unwrap();

    let mut output = [0.0; 8];
    engine.process(&mut output, 2);

    assert!(engine.transport().is_playing());
    assert!(output.iter().all(|sample| *sample > 0.0));

    let transition = std::iter::from_fn(|| events.pop().ok()).find_map(|event| match event {
        EngineEvent::SectionTransitioned {
            section_id: actual,
            effective_at_samples,
            retired,
        } => Some((actual, effective_at_samples, retired)),
        _ => None,
    });
    let (actual, effective_at_samples, retired) = transition.expect("transition event");
    assert_eq!(actual, section_id);
    assert_eq!(effective_at_samples, 0);
    assert_eq!(retired.tracks().len(), 1);
    assert!(retired.tracks()[0].source.clips.is_empty());
}

#[test]
fn refreshing_the_active_section_changes_content_without_restarting_its_playhead() {
    let track_id = TrackId::new();
    let (mut engine, mut commands, mut events) =
        graph_engine(&[routing_channel(track_id), routing_channel(TrackId::MASTER)]);
    let section_id = SectionId::new();
    commands.push(EngineCommand::SetSampleRate(8)).unwrap();
    commands.push(EngineCommand::add_track(track_id)).unwrap();

    let source = |value| {
        Box::new(PreparedSectionPlaybackSource::new(
            section_id,
            4.0,
            true,
            vec![(
                track_id,
                PreparedPlaybackSource::new(
                    vec![EngineClip {
                        id: ClipId::new(),
                        audio: constant_audio(32, value),
                        position: 0,
                        source_offset: 0,
                        start_marker: 0,
                        duration: 32,
                        loop_enabled: false,
                        loop_start: 0,
                        loop_end: 0,
                        linear_gain: 1.0,
                        fades: Default::default(),
                        playback_direction: Default::default(),
                        warp_markers: Default::default(),
                    }],
                    Vec::new(),
                    Vec::new(),
                ),
            )],
        ))
    };

    commands
        .push(EngineCommand::LaunchSection(source(0.25)))
        .unwrap();
    let mut before = [0.0; 4];
    engine.process(&mut before, 1);
    assert_eq!(before, [0.25; 4]);

    commands
        .push(EngineCommand::RefreshSection(source(0.75)))
        .unwrap();
    let mut after = [0.0; 4];
    engine.process(&mut after, 1);

    assert_eq!(after, [0.75; 4]);
    let position = engine
        .active_section
        .expect("Section remains active")
        .position_samples;
    assert_eq!(position, 8, "refresh preserves the four elapsed samples");
    let refresh = std::iter::from_fn(|| events.pop().ok()).find_map(|event| match event {
        EngineEvent::SectionSourceRefreshed {
            applied,
            effective_at_samples,
            section_position_samples,
            ..
        } => Some((applied, effective_at_samples, section_position_samples)),
        _ => None,
    });
    assert_eq!(refresh, Some((true, 4, Some(4))));
}

#[test]
fn section_wraps_at_its_shortened_boundary_inside_one_callback() {
    let track_id = TrackId::new();
    let (mut engine, mut commands, _events) =
        graph_engine(&[routing_channel(track_id), routing_channel(TrackId::MASTER)]);
    commands.push(EngineCommand::SetSampleRate(8)).unwrap();
    commands.push(EngineCommand::add_track(track_id)).unwrap();
    let audio = Arc::new(DecodedAudio {
        channels: vec![vec![0.1, 0.2, 0.3, 0.4, 0.9, 0.9]],
        sample_rate: 8,
    });
    commands
        .push(EngineCommand::LaunchSection(Box::new(
            PreparedSectionPlaybackSource::new(
                SectionId::new(),
                1.0,
                true,
                vec![(
                    track_id,
                    PreparedPlaybackSource::new(
                        vec![EngineClip {
                            id: ClipId::new(),
                            audio,
                            position: 0,
                            source_offset: 0,
                            start_marker: 0,
                            duration: 6,
                            loop_enabled: false,
                            loop_start: 0,
                            loop_end: 0,
                            linear_gain: 1.0,
                            fades: Default::default(),
                            playback_direction: Default::default(),
                            warp_markers: Default::default(),
                        }],
                        Vec::new(),
                        Vec::new(),
                    ),
                )],
            ),
        )))
        .unwrap();

    // 8 Hz at 120 BPM is four samples per beat, so this callback crosses the
    // shortened one-beat boundary after frame four.
    let mut output = [0.0; 6];
    engine.process(&mut output, 1);

    assert!((output[0] - output[4]).abs() < f32::EPSILON);
    assert!((output[1] - output[5]).abs() < f32::EPSILON);
    assert_ne!(output[2], output[4]);
}

#[test]
fn arrangement_loop_does_not_wrap_section_engine_time() {
    let track_id = TrackId::new();
    let (mut engine, mut commands, mut events) =
        graph_engine(&[routing_channel(track_id), routing_channel(TrackId::MASTER)]);
    let section_id = SectionId::new();
    commands.push(EngineCommand::SetSampleRate(8)).unwrap();
    commands.push(EngineCommand::add_track(track_id)).unwrap();
    commands
        .push(EngineCommand::SetArrangementLoopRegion { start: 0, end: 2 })
        .unwrap();
    commands
        .push(EngineCommand::SetArrangementLoop(true))
        .unwrap();
    commands
        .push(EngineCommand::LaunchSection(Box::new(
            PreparedSectionPlaybackSource::new(
                section_id,
                4.0,
                true,
                vec![(track_id, PreparedPlaybackSource::default())],
            ),
        )))
        .unwrap();

    engine.process(&mut [0.0; 3], 1);

    assert_eq!(engine.transport().position(), 0);
    let events: Vec<_> = std::iter::from_fn(|| events.pop().ok()).collect();
    assert!(events
        .iter()
        .any(|event| matches!(event, EngineEvent::PerformancePosition(3))));
    assert!(events.iter().any(|event| matches!(
        event,
        EngineEvent::SectionPlaybackPosition {
            section_id: actual,
            position_samples: 3,
        } if *actual == section_id
    )));
}

#[test]
fn non_looping_section_returns_to_arrangement_source_after_ending() {
    let track_id = TrackId::new();
    let (mut engine, mut commands, _events) =
        graph_engine(&[routing_channel(track_id), routing_channel(TrackId::MASTER)]);
    commands.push(EngineCommand::SetSampleRate(8)).unwrap();
    commands.push(EngineCommand::add_track(track_id)).unwrap();
    engine.process(&mut [], 1);
    *engine.tracks[0].playback_source = PreparedPlaybackSource::new(
        vec![EngineClip {
            id: ClipId::new(),
            audio: constant_audio(16, 0.75),
            position: 0,
            source_offset: 0,
            start_marker: 0,
            duration: 16,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            linear_gain: 1.0,
            fades: Default::default(),
            playback_direction: Default::default(),
            warp_markers: Default::default(),
        }],
        Vec::new(),
        Vec::new(),
    );
    commands
        .push(EngineCommand::LaunchSection(Box::new(
            PreparedSectionPlaybackSource::new(
                SectionId::new(),
                1.0,
                false,
                vec![(
                    track_id,
                    PreparedPlaybackSource::new(
                        vec![EngineClip {
                            id: ClipId::new(),
                            audio: constant_audio(4, 0.25),
                            position: 0,
                            source_offset: 0,
                            start_marker: 0,
                            duration: 4,
                            loop_enabled: false,
                            loop_start: 0,
                            loop_end: 0,
                            linear_gain: 1.0,
                            fades: Default::default(),
                            playback_direction: Default::default(),
                            warp_markers: Default::default(),
                        }],
                        Vec::new(),
                        Vec::new(),
                    ),
                )],
            ),
        )))
        .unwrap();

    let mut section_output = [0.0; 4];
    engine.process(&mut section_output, 1);
    assert!(section_output.iter().all(|sample| *sample == 0.25));
    assert!(!engine.transport().is_playing());

    commands.push(EngineCommand::Seek(0)).unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut arrangement_output = [0.0; 1];
    engine.process(&mut arrangement_output, 1);

    assert_eq!(arrangement_output, [0.75]);
}

#[test]
fn switching_sections_releases_notes_from_the_previous_source() {
    let track_id = TrackId::new();
    let (mut engine, mut commands, _events) =
        graph_engine(&[routing_channel(track_id), routing_channel(TrackId::MASTER)]);
    let note_events = Arc::new(Mutex::new(Vec::new()));
    commands.push(EngineCommand::SetSampleRate(8)).unwrap();
    commands.push(EngineCommand::add_track(track_id)).unwrap();
    commands
        .push(EngineCommand::SetInstrument {
            track_id,
            instrument: Box::new(NoteLogInstrument(Arc::clone(&note_events))),
        })
        .unwrap();
    commands
        .push(EngineCommand::LaunchSection(Box::new(
            PreparedSectionPlaybackSource::new(
                SectionId::new(),
                4.0,
                true,
                vec![(
                    track_id,
                    PreparedPlaybackSource::new(
                        Vec::new(),
                        vec![EngineNoteClip::new(
                            ClipId::new(),
                            0.0,
                            4.0,
                            vec![MidiNote {
                                pitch: 60,
                                velocity: 100,
                                start_beat: 0.0,
                                duration_beats: 4.0,
                            }],
                            0.0,
                            false,
                            0.0,
                            0.0,
                            vibez_core::perform::GrooveGrid::Off,
                        )],
                        Vec::new(),
                    ),
                )],
            ),
        )))
        .unwrap();
    let mut output = [0.0; 1];
    engine.process(&mut output, 1);
    assert!(note_events.lock().unwrap().contains(&(true, 60)));

    commands
        .push(EngineCommand::LaunchSection(Box::new(
            PreparedSectionPlaybackSource::new(
                SectionId::new(),
                4.0,
                true,
                vec![(track_id, PreparedPlaybackSource::default())],
            ),
        )))
        .unwrap();
    engine.process(&mut output, 1);

    assert!(note_events.lock().unwrap().contains(&(false, 60)));
}

#[test]
fn live_instrument_note_releases_after_a_section_transition() {
    let track_id = TrackId::new();
    let (mut engine, mut commands, _events) =
        graph_engine(&[routing_channel(track_id), routing_channel(TrackId::MASTER)]);
    let note_events = Arc::new(Mutex::new(Vec::new()));
    commands.push(EngineCommand::add_track(track_id)).unwrap();
    commands
        .push(EngineCommand::SetInstrument {
            track_id,
            instrument: Box::new(NoteLogInstrument(Arc::clone(&note_events))),
        })
        .unwrap();
    commands
        .push(EngineCommand::LaunchSection(Box::new(
            PreparedSectionPlaybackSource::new(
                SectionId::new(),
                4.0,
                true,
                vec![(track_id, PreparedPlaybackSource::default())],
            ),
        )))
        .unwrap();
    commands
        .push(EngineCommand::ExternalNoteOn {
            track_id,
            pitch: 48,
            velocity: 100,
        })
        .unwrap();
    let mut output = [0.0; 1];
    engine.process(&mut output, 1);

    commands
        .push(EngineCommand::LaunchSection(Box::new(
            PreparedSectionPlaybackSource::new(
                SectionId::new(),
                4.0,
                true,
                vec![(track_id, PreparedPlaybackSource::default())],
            ),
        )))
        .unwrap();
    engine.process(&mut output, 1);
    commands
        .push(EngineCommand::ExternalNoteOff {
            track_id,
            pitch: 48,
        })
        .unwrap();
    engine.process(&mut output, 1);

    assert_eq!(*note_events.lock().unwrap(), vec![(true, 48), (false, 48)]);
}

#[test]
fn absent_track_stops_source_content_without_resetting_effect_tail() {
    let track_id = TrackId::new();
    let (mut engine, mut commands, _events) =
        graph_engine(&[routing_channel(track_id), routing_channel(TrackId::MASTER)]);
    commands.push(EngineCommand::add_track(track_id)).unwrap();
    let effect_id = EffectId::new();
    commands
        .push(EngineCommand::InsertEffect {
            track_id,
            effect_id,
            effect: Box::new(TailEffect { memory: 0.0 }),
            position: None,
        })
        .unwrap();
    commands
        .push(EngineCommand::SetRouting(routing_plan(&[
            RoutingChannel {
                effects: vec![routing_effect(effect_id)],
                ..routing_channel(track_id)
            },
            routing_channel(TrackId::MASTER),
        ])))
        .unwrap();
    commands
        .push(EngineCommand::LaunchSection(Box::new(
            PreparedSectionPlaybackSource::new(
                SectionId::new(),
                4.0,
                true,
                vec![(
                    track_id,
                    PreparedPlaybackSource::new(
                        vec![EngineClip {
                            id: ClipId::new(),
                            audio: constant_audio(1, 1.0),
                            position: 0,
                            source_offset: 0,
                            start_marker: 0,
                            duration: 1,
                            loop_enabled: false,
                            loop_start: 0,
                            loop_end: 0,
                            linear_gain: 1.0,
                            fades: Default::default(),
                            playback_direction: Default::default(),
                            warp_markers: Default::default(),
                        }],
                        Vec::new(),
                        Vec::new(),
                    ),
                )],
            ),
        )))
        .unwrap();
    let mut source = [0.0; 1];
    engine.process(&mut source, 1);
    assert!(source[0] > 0.0);

    commands
        .push(EngineCommand::LaunchSection(Box::new(
            PreparedSectionPlaybackSource::new(
                SectionId::new(),
                4.0,
                true,
                vec![(track_id, PreparedPlaybackSource::default())],
            ),
        )))
        .unwrap();
    let mut silence_with_tail = [0.0; 2];
    engine.process(&mut silence_with_tail, 1);

    assert!(silence_with_tail.iter().all(|sample| *sample > 0.0));
    assert!(silence_with_tail[0] < source[0]);
    assert!(silence_with_tail[1] < silence_with_tail[0]);
}

struct SectionRefreshFixture {
    engine: AudioEngine,
    commands: Producer<EngineCommand>,
    events: Consumer<EngineEvent>,
    track_id: TrackId,
    section_id: SectionId,
    old_owner: std::sync::Weak<DecodedAudio>,
}

fn section_refresh_source(
    track_id: TrackId,
    section_id: SectionId,
    audio: Arc<DecodedAudio>,
) -> Box<PreparedSectionPlaybackSource> {
    Box::new(PreparedSectionPlaybackSource::new(
        section_id,
        32.0,
        true,
        vec![(
            track_id,
            PreparedPlaybackSource::new(
                vec![EngineClip {
                    id: ClipId::new(),
                    audio,
                    position: 0,
                    source_offset: 0,
                    start_marker: 0,
                    duration: 4096,
                    loop_enabled: false,
                    loop_start: 0,
                    loop_end: 0,
                    linear_gain: 1.0,
                    fades: Default::default(),
                    playback_direction: Default::default(),
                    warp_markers: Default::default(),
                }],
                Vec::new(),
                Vec::new(),
            ),
        )],
    ))
}

fn section_refresh_fixture(delay: u32) -> SectionRefreshFixture {
    use vibez_core::routing::{NodeStage, RoutingChannel, RoutingEffect, RoutingNode};

    let (engine, mut commands, events) = AudioEngine::new();
    let track_id = TrackId::new();
    let effect_id = EffectId::new();
    let section_id = SectionId::new();
    commands.push(EngineCommand::SetSampleRate(1000)).unwrap();
    commands.push(EngineCommand::SetBpm(60.0)).unwrap();
    commands.push(EngineCommand::add_track(track_id)).unwrap();
    commands
        .push(EngineCommand::SetTrackPan(track_id, 0.0))
        .unwrap();
    commands
        .push(EngineCommand::InsertEffect {
            track_id,
            effect_id,
            effect: Box::new(crate::test_support::DelayProbe::new(delay)),
            position: None,
        })
        .unwrap();
    let channels = [
        RoutingChannel {
            id: track_id,
            is_bus: false,
            sends: vec![],
            effects: vec![RoutingEffect {
                id: effect_id,
                inputs: vec![],
                assignments: vec![],
                inactive_inputs: vec![],
            }],
        },
        RoutingChannel {
            id: TrackId::MASTER,
            is_bus: true,
            effects: vec![],
            sends: vec![],
        },
    ];
    let reports = [(
        RoutingNode {
            channel: track_id,
            stage: NodeStage::Effect(effect_id),
        },
        delay,
    )];
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare_compensated(&channels, 32, &reports, &[], 1)
                .unwrap(),
        ))
        .unwrap();
    let old = constant_audio(4096, 0.25);
    let old_owner = Arc::downgrade(&old);
    commands
        .push(EngineCommand::LaunchSection(section_refresh_source(
            track_id, section_id, old,
        )))
        .unwrap();
    SectionRefreshFixture {
        engine,
        commands,
        events,
        track_id,
        section_id,
        old_owner,
    }
}

fn assert_refresh_left_audio(output: &[f32; 64], expected: f32) {
    for [left, right] in output.as_chunks::<2>().0 {
        assert_eq!(*left, expected);
        assert_eq!(*right, 0.0);
    }
}

fn process_refresh_block(engine: &mut AudioEngine, output: &mut [f32; 64]) {
    assert_eq!(
        crate::retirement::tests::allocations(
            || engine.process_block(AudioProcessBlock::new(output, 2))
        ),
        (0, 0)
    );
}

fn warm_section_refresh(fixture: &mut SectionRefreshFixture) {
    let mut output = [0.0; 64];
    for _ in 0..3 {
        fixture
            .engine
            .process_block(AudioProcessBlock::new(&mut output, 2));
        while fixture.events.pop().is_ok() {}
    }
    assert_refresh_left_audio(&output, 0.25);
    assert_eq!(fixture.engine.active_section.unwrap().position_samples, 96);
    assert_eq!(fixture.old_owner.strong_count(), 1);
}

#[test]
fn delayed_section_refresh_acknowledges_consumption_before_heard_capture_and_preserves_audio_history(
) {
    let SectionRefreshFixture {
        mut engine,
        mut commands,
        mut events,
        track_id,
        section_id,
        old_owner,
    } = section_refresh_fixture(64);
    let mut output = [0.0; 64];
    for block in 0..3 {
        engine.process_block(AudioProcessBlock::new(&mut output, 2));
        for [left, right] in output.as_chunks::<2>().0 {
            assert_eq!(
                *left,
                if block < 2 { 0.0 } else { 0.25 },
                "initial delayed source, block {block}"
            );
            assert_eq!(*right, 0.0);
        }
        while events.pop().is_ok() {}
    }
    assert_eq!(engine.active_section.unwrap().position_samples, 96);
    assert_eq!(old_owner.strong_count(), 1);
    let incoming = constant_audio(4096, 0.75);
    let incoming_owner = Arc::downgrade(&incoming);
    let replacement = section_refresh_source(track_id, section_id, incoming);
    let replacement_pointer = &*replacement as *const PreparedSectionPlaybackSource;
    commands
        .push(EngineCommand::RefreshSection(replacement))
        .unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(
            || engine.process_block(AudioProcessBlock::new(&mut output, 2))
        ),
        (0, 0)
    );
    for [left, right] in output.as_chunks::<2>().0 {
        assert_eq!(
            *left, 0.25,
            "already rendered old audio still occupies the real delay line"
        );
        assert_eq!(*right, 0.0);
    }
    assert_eq!(engine.active_section.unwrap().position_samples, 128);
    assert_eq!(engine.active_section.unwrap().section_id, section_id);
    assert!(engine.transport().is_playing());
    let mut returned = None;
    while let Ok(event) = events.pop() {
        assert!(
            !matches!(
                event,
                EngineEvent::SectionCaptureSource {
                    refreshed: true,
                    ..
                }
            ),
            "Capture still waits for the original 64-frame deadline"
        );
        assert!(
            !matches!(event, EngineEvent::SectionTransitioned { .. }),
            "refresh does not relaunch the Section"
        );
        if let EngineEvent::SectionSourceRefreshed {
            section_id: actual_section,
            applied,
            effective_at_samples,
            section_position_samples,
            retired,
        } = event
        {
            assert!(returned.is_none(), "consumption is acknowledged once");
            assert_eq!(actual_section, section_id);
            assert!(applied);
            assert_eq!(effective_at_samples, 96);
            assert_eq!(section_position_samples, Some(96));
            assert!(std::ptr::eq(&*retired, replacement_pointer));
            assert_eq!(retired.tracks().len(), 1);
            assert_eq!(
                Arc::as_ptr(&retired.tracks()[0].source.clips[0].audio),
                old_owner.as_ptr()
            );
            returned = Some(retired);
        }
    }
    let returned = returned.expect("the owner acknowledgement does not wait for output latency");
    assert_eq!(old_owner.strong_count(), 1);
    assert_eq!(incoming_owner.strong_count(), 1);
    drop(returned);
    assert_eq!(
        old_owner.strong_count(),
        0,
        "UI disposal may precede the old delayed samples"
    );

    assert_eq!(
        crate::retirement::tests::allocations(
            || engine.process_block(AudioProcessBlock::new(&mut output, 2))
        ),
        (0, 0)
    );
    for [left, right] in output.as_chunks::<2>().0 {
        assert_eq!(
            *left, 0.25,
            "delay history survives destruction of its former source owner"
        );
        assert_eq!(*right, 0.0);
    }
    let notifications: Vec<_> = std::iter::from_fn(|| events.pop().ok()).collect();
    assert!(!notifications
        .iter()
        .any(|event| matches!(event, EngineEvent::SectionSourceRefreshed { .. })));
    let capture: Vec<_> = notifications
        .iter()
        .filter_map(|event| match event {
            EngineEvent::SectionCaptureSource {
                section_id,
                effective_at_samples,
                section_position_samples,
                refreshed,
                offsets,
            } => Some((
                *section_id,
                *effective_at_samples,
                *section_position_samples,
                *refreshed,
                offsets.is_empty(),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        capture,
        [(section_id, 96, 96, true, true)],
        "Capture arrives only when the original refresh becomes audible"
    );
    assert_eq!(engine.active_section.unwrap().position_samples, 160);

    assert_eq!(
        crate::retirement::tests::allocations(
            || engine.process_block(AudioProcessBlock::new(&mut output, 2))
        ),
        (0, 0)
    );
    for [left, right] in output.as_chunks::<2>().0 {
        assert_eq!(
            *left, 0.75,
            "replacement reaches the speakers after the real 64-frame delay"
        );
        assert_eq!(*right, 0.0);
    }
    assert_eq!(engine.active_section.unwrap().position_samples, 192);
    assert_eq!(old_owner.strong_count(), 0);
    assert_eq!(incoming_owner.strong_count(), 1);
    while let Ok(event) = events.pop() {
        assert!(!matches!(
            event,
            EngineEvent::SectionSourceRefreshed { .. }
                | EngineEvent::SectionCaptureSource {
                    refreshed: true,
                    ..
                }
        ));
    }
}

#[derive(Clone, Copy, Debug)]
enum SectionRefreshTarget {
    Active,
    DifferentSection,
    Stopped,
}

#[test]
fn section_refresh_acknowledgement_state_matrix_returns_displaced_or_rejected_owners_without_latency(
) {
    for delay in [0, 64] {
        for target in [
            SectionRefreshTarget::Active,
            SectionRefreshTarget::DifferentSection,
            SectionRefreshTarget::Stopped,
        ] {
            let mut fixture = section_refresh_fixture(delay);
            warm_section_refresh(&mut fixture);
            if matches!(target, SectionRefreshTarget::Stopped) {
                fixture.commands.push(EngineCommand::Stop).unwrap();
                fixture
                    .engine
                    .process_block(AudioProcessBlock::new(&mut [], 2));
                while fixture.events.pop().is_ok() {}
            }
            let requested = if matches!(target, SectionRefreshTarget::DifferentSection) {
                SectionId::new()
            } else {
                fixture.section_id
            };
            let expected_effective = fixture.engine.effective_position();
            let expected_local = fixture
                .engine
                .active_section
                .map(|active| active.position_samples);
            let applied = matches!(target, SectionRefreshTarget::Active);
            let incoming = constant_audio(4096, 0.75);
            let incoming_owner = Arc::downgrade(&incoming);
            let prepared = section_refresh_source(fixture.track_id, requested, incoming);
            let pointer = &*prepared as *const PreparedSectionPlaybackSource;
            fixture
                .commands
                .push(EngineCommand::RefreshSection(prepared))
                .unwrap();
            let mut output = [0.0; 64];
            process_refresh_block(&mut fixture.engine, &mut output);
            if !matches!(target, SectionRefreshTarget::Stopped) {
                assert_refresh_left_audio(&output, if applied && delay == 0 { 0.75 } else { 0.25 });
                assert_eq!(
                    fixture.engine.active_section.unwrap().section_id,
                    fixture.section_id
                );
                assert_eq!(fixture.engine.active_section.unwrap().position_samples, 128);
                assert!(fixture.engine.transport().is_playing());
            } else {
                assert!(fixture.engine.active_section.is_none());
                assert!(!fixture.engine.transport().is_playing());
            }
            let notifications: Vec<_> = std::iter::from_fn(|| fixture.events.pop().ok()).collect();
            let returned: Vec<_> = notifications
                .iter()
                .filter_map(|event| match event {
                    EngineEvent::SectionSourceRefreshed {
                        section_id,
                        applied: actual_applied,
                        effective_at_samples,
                        section_position_samples,
                        retired,
                    } => {
                        assert_eq!(event.time_domain(), crate::events::TimeDomain::Untimed);
                        assert_eq!(*section_id, requested);
                        assert_eq!(*actual_applied, applied, "delay {delay}, target {target:?}");
                        assert_eq!(*effective_at_samples, expected_effective);
                        assert_eq!(
                            *section_position_samples,
                            if applied { expected_local } else { None }
                        );
                        assert!(std::ptr::eq(&**retired, pointer));
                        Some(retired)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(
                returned.len(),
                1,
                "same-callback ack for delay {delay}, target {target:?}"
            );
            let returned_audio = Arc::as_ptr(&returned[0].tracks()[0].source.clips[0].audio);
            assert_eq!(
                returned_audio,
                if applied {
                    fixture.old_owner.as_ptr()
                } else {
                    incoming_owner.as_ptr()
                }
            );
            let resident_audio =
                Arc::as_ptr(&fixture.engine.tracks()[0].section_playback_source.clips[0].audio);
            assert_eq!(
                resident_audio,
                if applied {
                    incoming_owner.as_ptr()
                } else {
                    fixture.old_owner.as_ptr()
                }
            );
            assert_eq!(
                notifications
                    .iter()
                    .filter(|event| matches!(
                        event,
                        EngineEvent::SectionCaptureSource {
                            refreshed: true,
                            ..
                        }
                    ))
                    .count(),
                usize::from(applied && delay == 0)
            );
            assert_eq!(fixture.old_owner.strong_count(), 1);
            assert_eq!(incoming_owner.strong_count(), 1);
            drop(returned);
            drop(notifications);
            assert_eq!(fixture.old_owner.strong_count(), usize::from(!applied));
            assert_eq!(incoming_owner.strong_count(), usize::from(applied));
        }
    }
}

#[test]
fn section_refresh_full_ui_ring_retains_unique_owner_and_allows_fifo_command_progress() {
    let mut fixture = section_refresh_fixture(64);
    warm_section_refresh(&mut fixture);
    let ring_capacity = fixture.engine.event_tx.slots();
    for _ in 0..ring_capacity {
        fixture
            .engine
            .event_tx
            .push(EngineEvent::PlaybackStarted)
            .unwrap();
    }
    assert!(fixture.engine.event_tx.is_full());
    let presentation_capacity = fixture.engine.scheduled_presentation.capacity();
    let retirement_capacity = fixture.engine.pending_retirements.capacity();
    let incoming = constant_audio(4096, 0.75);
    let incoming_owner = Arc::downgrade(&incoming);
    let prepared = section_refresh_source(fixture.track_id, fixture.section_id, incoming);
    let pointer = &*prepared as *const PreparedSectionPlaybackSource;
    fixture
        .commands
        .push(EngineCommand::RefreshSection(prepared))
        .unwrap();
    let swing = vibez_core::perform::SwingAmount::new(0.625);
    fixture
        .commands
        .push(EngineCommand::SetProjectSwing(swing))
        .unwrap();
    let mut output = [0.0; 64];
    process_refresh_block(&mut fixture.engine, &mut output);
    assert_refresh_left_audio(&output, 0.25);
    assert_eq!(fixture.engine.project_swing, swing);
    assert!(
        fixture.engine.cmd_rx.peek().is_err(),
        "a held owner return cannot retain command admission credits"
    );
    assert_eq!(fixture.engine.active_section.unwrap().position_samples, 128);
    assert_eq!(fixture.old_owner.strong_count(), 1);
    assert_eq!(incoming_owner.strong_count(), 1);
    assert_eq!(
        Arc::as_ptr(&fixture.engine.tracks()[0].section_playback_source.clips[0].audio),
        incoming_owner.as_ptr()
    );
    assert_eq!(fixture.engine.scheduled_presentation.iter().filter(|pending| matches!(&pending.event, EngineEvent::SectionSourceRefreshed { retired, .. } if std::ptr::eq(&**retired, pointer))).count(), 1);
    assert_eq!(
        fixture.engine.scheduled_presentation.capacity(),
        presentation_capacity
    );
    assert_eq!(
        fixture.engine.pending_retirements.capacity(),
        retirement_capacity
    );
    let mut markers = 0;
    while let Ok(event) = fixture.events.pop() {
        assert!(matches!(event, EngineEvent::PlaybackStarted));
        markers += 1;
    }
    assert_eq!(markers, ring_capacity);
    assert_eq!(
        crate::retirement::tests::allocations(|| fixture
            .engine
            .process_block(AudioProcessBlock::new(&mut [], 2))),
        (0, 0)
    );
    let notifications: Vec<_> = std::iter::from_fn(|| fixture.events.pop().ok()).collect();
    let acknowledgements: Vec<_> = notifications
        .iter()
        .filter_map(|event| match event {
            EngineEvent::SectionSourceRefreshed {
                applied: true,
                effective_at_samples: 96,
                section_position_samples: Some(96),
                retired,
                ..
            } => Some(retired),
            _ => None,
        })
        .collect();
    assert_eq!(acknowledgements.len(), 1);
    assert!(std::ptr::eq(&**acknowledgements[0], pointer));
    assert!(!notifications.iter().any(|event| matches!(
        event,
        EngineEvent::SectionCaptureSource {
            refreshed: true,
            ..
        }
    )));
    drop(acknowledgements);
    drop(notifications);
    assert_eq!(fixture.old_owner.strong_count(), 0);
    process_refresh_block(&mut fixture.engine, &mut output);
    assert_refresh_left_audio(&output, 0.25);
    let capture: Vec<_> = std::iter::from_fn(|| fixture.events.pop().ok())
        .filter(|event| {
            matches!(
                event,
                EngineEvent::SectionCaptureSource {
                    refreshed: true,
                    effective_at_samples: 96,
                    section_position_samples: 96,
                    ..
                }
            )
        })
        .collect();
    assert_eq!(capture.len(), 1);
    process_refresh_block(&mut fixture.engine, &mut output);
    assert_refresh_left_audio(&output, 0.75);
    assert_eq!(
        fixture.engine.scheduled_presentation.capacity(),
        presentation_capacity
    );
    assert_eq!(
        fixture.engine.pending_retirements.capacity(),
        retirement_capacity
    );
    while let Ok(event) = fixture.events.pop() {
        assert!(!matches!(
            event,
            EngineEvent::SectionSourceRefreshed { .. }
                | EngineEvent::SectionCaptureSource {
                    refreshed: true,
                    ..
                }
        ));
    }
}

#[test]
fn section_refresh_then_public_capture_stop_preserves_owner_and_original_late_feed_deadline() {
    let mut fixture = section_refresh_fixture(64);
    warm_section_refresh(&mut fixture);
    fixture
        .commands
        .push(EngineCommand::StartPerformanceCapture)
        .unwrap();
    let mut output = [0.0; 64];
    process_refresh_block(&mut fixture.engine, &mut output);
    assert!(fixture.engine.capture_active);
    while fixture.events.pop().is_ok() {}
    let ring_capacity = fixture.engine.event_tx.slots();
    for _ in 0..ring_capacity {
        fixture
            .engine
            .event_tx
            .push(EngineEvent::PlaybackStarted)
            .unwrap();
    }
    let incoming = constant_audio(4096, 0.75);
    let prepared = section_refresh_source(fixture.track_id, fixture.section_id, incoming);
    let pointer = &*prepared as *const PreparedSectionPlaybackSource;
    let stop_boundary = fixture.engine.heard_capture_position();
    fixture
        .commands
        .push(EngineCommand::RefreshSection(prepared))
        .unwrap();
    fixture
        .commands
        .push(EngineCommand::StopPerformanceCapture)
        .unwrap();
    process_refresh_block(&mut fixture.engine, &mut output);
    assert_refresh_left_audio(&output, 0.25);
    assert!(!fixture.engine.capture_active);
    assert!(fixture.engine.transport().is_playing());
    assert_eq!(fixture.engine.active_section.unwrap().position_samples, 160);
    assert_eq!(fixture.old_owner.strong_count(), 1);
    assert!(
        fixture
            .engine
            .scheduled_presentation
            .iter()
            .any(|pending| pending.due == 192
                && matches!(
                    pending.event,
                    EngineEvent::SectionCaptureSource {
                        refreshed: true,
                        effective_at_samples: 128,
                        section_position_samples: 128,
                        ..
                    }
                )),
        "public Capture stop preserves the already accepted timed feed"
    );
    let mut markers = 0;
    while let Ok(event) = fixture.events.pop() {
        assert!(matches!(event, EngineEvent::PlaybackStarted));
        markers += 1;
    }
    assert_eq!(markers, ring_capacity);
    assert_eq!(
        crate::retirement::tests::allocations(|| fixture
            .engine
            .process_block(AudioProcessBlock::new(&mut [], 2))),
        (0, 0)
    );
    let notifications: Vec<_> = std::iter::from_fn(|| fixture.events.pop().ok()).collect();
    assert_eq!(notifications.iter().filter(|event| matches!(event, EngineEvent::SectionSourceRefreshed { applied: true, retired, .. } if std::ptr::eq(&**retired, pointer))).count(), 1);
    let stops: Vec<_> = notifications
        .iter()
        .filter_map(|event| match event {
            EngineEvent::PerformanceCaptureStopped {
                effective_at_samples,
            } => Some(*effective_at_samples),
            _ => None,
        })
        .collect();
    assert_eq!(stops, [stop_boundary]);
    assert!(!notifications.iter().any(|event| matches!(
        event,
        EngineEvent::SectionCaptureSource {
            refreshed: true,
            ..
        }
    )));
    drop(notifications);
    assert_eq!(fixture.old_owner.strong_count(), 0);
    process_refresh_block(&mut fixture.engine, &mut output);
    assert_refresh_left_audio(&output, 0.25);
    let notifications: Vec<_> = std::iter::from_fn(|| fixture.events.pop().ok()).collect();
    assert_eq!(
        notifications
            .iter()
            .filter(|event| matches!(
                event,
                EngineEvent::SectionCaptureSource {
                    refreshed: true,
                    effective_at_samples: 128,
                    section_position_samples: 128,
                    ..
                }
            ))
            .count(),
        1
    );
    assert!(!notifications.iter().any(|event| matches!(
        event,
        EngineEvent::PerformanceCaptureStarted { .. } | EngineEvent::SectionSourceRefreshed { .. }
    )));
    process_refresh_block(&mut fixture.engine, &mut output);
    assert_refresh_left_audio(&output, 0.75);
    assert!(!fixture.engine.capture_active);
    assert_eq!(fixture.engine.active_section.unwrap().position_samples, 224);
}
