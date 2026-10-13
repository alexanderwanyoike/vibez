//! Controlled Source automation clears absent values across actual timeline edits.

use super::*;
use crate::playback_source::{PreparedPlaybackSource, PreparedSectionPlaybackSource};
use vibez_core::{
    automation::{AutomationLane, AutomationPoint, AutomationTarget},
    id::{SectionId, TrackId},
    perform::{SwingAmount, SwingOffset},
    routing::*,
};

fn swing_lane() -> AutomationLane {
    let mut lane = AutomationLane::new(AutomationTarget::TrackSwingOffset);
    lane.insert_point(AutomationPoint {
        beat: 0.0,
        value: SwingOffset::new(0.1).normalized(),
        curve: 0.0,
    });
    lane
}
fn setup(
    lane: AutomationLane,
) -> (
    AudioEngine,
    Producer<EngineCommand>,
    Consumer<EngineEvent>,
    TrackId,
) {
    let (mut engine, mut commands, events) = AudioEngine::new();
    let id = TrackId::new();
    engine.sample_rate = 128;
    engine.transport.set_bpm(60.0);
    let mut source = PreparedPlaybackSource::default();
    source.automation.push(lane);
    engine
        .tracks
        .push(EngineTrack::with_playback_source(id, source));
    let channels = [
        RoutingChannel {
            id,
            is_bus: false,
            sends: vec![],
            effects: vec![],
        },
        RoutingChannel {
            id: TrackId::MASTER,
            is_bus: true,
            sends: vec![],
            effects: vec![],
        },
    ];
    let mut plan =
        crate::routing::PreparedRouting::prepare_compensated(&channels, 16, &[], &[], 1).unwrap();
    plan.configure_automation(&[(id, AutomationTarget::TrackSwingOffset)])
        .unwrap();
    commands.push(EngineCommand::SetRouting(plan)).unwrap();
    commands.push(EngineCommand::Play).unwrap();
    engine.process(&mut [0.0; 32], 2);
    (engine, commands, events, id)
}
#[test]
fn deleting_a_swing_lane_restores_manual_source_swing_on_the_controlled_graph() {
    let lane = swing_lane();
    let lane_id = lane.id;
    let old_points = lane.points.as_ptr();
    let (mut engine, mut commands, mut events, id) = setup(lane);
    engine.tracks[0].swing_offset = Some(SwingOffset::new(-0.05));
    assert!(
        (engine.tracks[0]
            .effective_swing(SwingAmount::new(0.56))
            .get()
            - 0.66)
            .abs()
            < 1e-6
    );
    while events.pop().is_ok() {}
    while engine
        .event_tx
        .push(EngineEvent::PlaybackPosition(0))
        .is_ok()
    {}
    commands
        .push(EngineCommand::RemoveAutomationLane {
            track_id: id,
            lane_id,
        })
        .unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 32], 2)),
        (0, 0)
    );
    assert!(
        (engine.tracks[0]
            .effective_swing(SwingAmount::new(0.56))
            .get()
            - 0.51)
            .abs()
            < 1e-6
    );
    assert_eq!(engine.pending_retirements.len(), 1);
    while events.pop().is_ok() {}
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.flush_retirements()),
        (0, 0)
    );
    let old = std::iter::from_fn(|| events.pop().ok())
        .find_map(|event| match event {
            EngineEvent::RetiredAutomationLane(lane) => Some(lane),
            _ => None,
        })
        .unwrap();
    assert_eq!(old.id, lane_id);
    assert_eq!(old.points.as_ptr(), old_points);
    drop(old);
}
#[test]
fn a_section_without_swing_does_not_inherit_the_previous_sections_offset() {
    let (mut engine, mut commands, mut events, id) = setup(swing_lane());
    for (with_lane, expected) in [(true, 0.66), (false, 0.56)] {
        let mut source = PreparedPlaybackSource::default();
        if with_lane {
            source.automation.push(swing_lane());
        }
        commands
            .push(EngineCommand::LaunchSection(Box::new(
                PreparedSectionPlaybackSource::new(SectionId::new(), 8.0, true, vec![(id, source)]),
            )))
            .unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 32], 2)),
            (0, 0)
        );
        while events.pop().is_ok() {}
        assert!(
            (engine.tracks[0]
                .effective_swing(SwingAmount::new(0.56))
                .get()
                - expected)
                .abs()
                < 1e-6
        );
    }
}

#[test]
fn graph_control_visits_are_linear_in_configured_controls_not_channels_times_controls() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let ids: Vec<_> = (0..12).map(|_| TrackId::new()).collect();
    let mut channels = Vec::new();
    let mut targets = Vec::new();
    for &id in &ids {
        engine.tracks.push(EngineTrack::new(id));
        channels.push(RoutingChannel {
            id,
            is_bus: false,
            sends: vec![],
            effects: vec![],
        });
        targets.push((id, AutomationTarget::TrackGain));
    }
    channels.push(RoutingChannel {
        id: TrackId::MASTER,
        is_bus: true,
        sends: vec![],
        effects: vec![],
    });
    let mut plan =
        crate::routing::PreparedRouting::prepare_compensated(&channels, 16, &[], &[], 1).unwrap();
    plan.configure_automation(&targets).unwrap();
    commands.push(EngineCommand::SetRouting(plan)).unwrap();
    commands.push(EngineCommand::Play).unwrap();
    super::graph_render::take_control_scan_visits();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 32], 2)),
        (0, 0)
    );
    assert_eq!(
        super::graph_render::take_control_scan_visits(),
        targets.len()
    );
    while events.pop().is_ok() {}
}
#[test]
fn static_send_gains_do_not_scan_automation_per_audio_frame() {
    let (mut engine, mut commands, _) = AudioEngine::new();
    let track = TrackId::new();
    let bus = TrackId::new();
    let mut source = EngineTrack::new(track);
    source.sends.push((bus, 0.5));
    engine.tracks.push(source);
    engine.buses.push(EngineTrack::new(bus));
    let channels = [
        RoutingChannel {
            id: track,
            is_bus: false,
            sends: vec![bus],
            effects: vec![],
        },
        RoutingChannel {
            id: bus,
            is_bus: true,
            sends: vec![],
            effects: vec![],
        },
        RoutingChannel {
            id: TrackId::MASTER,
            is_bus: true,
            sends: vec![],
            effects: vec![],
        },
    ];
    let plan =
        crate::routing::PreparedRouting::prepare_compensated(&channels, 16, &[], &[], 1).unwrap();
    commands.push(EngineCommand::SetRouting(plan)).unwrap();
    commands.push(EngineCommand::Play).unwrap();
    EngineTrack::take_send_scan_calls();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 32], 2)),
        (0, 0)
    );
    assert!(EngineTrack::take_send_scan_calls() <= 1);
}

#[test]
fn non_finite_swing_points_restore_the_manual_value_and_retire_replaced_storage() {
    let lane = swing_lane();
    let mut edited = lane.clone();
    let old_points = lane.points.as_ptr();
    let (mut engine, mut commands, mut events, id) = setup(lane);
    while events.pop().is_ok() {}
    engine.tracks[0].swing_offset = Some(SwingOffset::new(-0.05));
    edited.points[0].value = f32::NAN;
    commands
        .push(EngineCommand::SetAutomationLane {
            track_id: id,
            lane: edited,
        })
        .unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 32], 2)),
        (0, 0)
    );
    assert!(
        (engine.tracks[0]
            .effective_swing(SwingAmount::new(0.56))
            .get()
            - 0.51)
            .abs()
            < 1e-6
    );
    let old = std::iter::from_fn(|| events.pop().ok())
        .find_map(|event| match event {
            EngineEvent::RetiredAutomationLane(lane) => Some(lane),
            _ => None,
        })
        .unwrap();
    assert_eq!(old.points.as_ptr(), old_points);
    drop(old);
}

#[test]
fn prepared_and_new_send_automation_keep_sample_exact_ramps() {
    use crate::playback_source::EngineClip;
    use vibez_core::id::ClipId;
    fn render(prepared: bool, automated: bool) -> [f32; 32] {
        let (mut engine, mut commands, _) = AudioEngine::new();
        engine.sample_rate = 128;
        engine.transport.set_bpm(60.0);
        let track = TrackId::new();
        let bus = TrackId::new();
        let mut source = EngineTrack::new(track);
        source.sends.push((bus, 0.5));
        source.playback_source.clips.push(EngineClip {
            id: ClipId::new(),
            audio: Arc::new(DecodedAudio {
                channels: vec![vec![1.0; 128]; 2],
                sample_rate: 128,
            }),
            position: 0,
            source_offset: 0,
            start_marker: 0,
            duration: 128,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 128,
            linear_gain: 1.0,
            fades: Default::default(),
            playback_direction: Default::default(),
            warp_markers: Default::default(),
        });
        let target = AutomationTarget::Send { bus_id: bus };
        if automated {
            let mut lane = AutomationLane::new(target);
            for (beat, value) in [(0.0, 0.5), (1.0, 1.0)] {
                lane.insert_point(AutomationPoint {
                    beat,
                    value,
                    curve: 0.0,
                });
            }
            source.playback_source.automation.push(lane);
        }
        engine.tracks.push(source);
        engine.buses.push(EngineTrack::new(bus));
        let channels = [
            RoutingChannel {
                id: track,
                is_bus: false,
                sends: vec![bus],
                effects: vec![],
            },
            RoutingChannel {
                id: bus,
                is_bus: true,
                sends: vec![],
                effects: vec![],
            },
            RoutingChannel {
                id: TrackId::MASTER,
                is_bus: true,
                sends: vec![],
                effects: vec![],
            },
        ];
        let mut plan =
            crate::routing::PreparedRouting::prepare_compensated(&channels, 16, &[], &[], 1)
                .unwrap();
        if prepared {
            plan.configure_automation(&[(track, target)]).unwrap();
        }
        commands.push(EngineCommand::SetRouting(plan)).unwrap();
        commands.push(EngineCommand::Play).unwrap();
        let mut output = [0.0; 32];
        let mut captured = [0.0; 32];
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process_block(
                AudioProcessBlock::new(&mut output, 2)
                    .with_track_output_capture(bus.raw(), &mut captured)
            )),
            (0, 0)
        );
        captured
    }
    let baseline = render(false, false);
    assert!(baseline[0] > 0.1);
    for prepared in [false, true] {
        let automated = render(prepared, true);
        for frame in 0..16 {
            for channel in 0..2 {
                let expected = baseline[frame * 2 + channel] * (1.0 + frame as f32 / 128.0);
                assert!((automated[frame * 2 + channel] - expected).abs() < 1e-6);
            }
        }
    }
}

#[test]
fn deleted_swing_changes_future_repeat_pairs_without_moving_the_inflight_pair() {
    use vibez_core::{midi::InstrumentKind, perform::NoteRepeatRate};
    let lane = swing_lane();
    let lane_id = lane.id;
    let (mut engine, mut commands, mut events, id) = setup(lane);
    engine.tracks[0].instrument = Some(create_instrument(InstrumentKind::SubtractiveSynth, 128.0));
    commands
        .push(EngineCommand::SetProjectSwing(SwingAmount::new(0.56)))
        .unwrap();
    commands
        .push(EngineCommand::StartNoteRepeat {
            id: 0,
            track_id: id,
            pitch: 42,
            velocity: 100,
            rate: NoteRepeatRate::Eighth,
        })
        .unwrap();
    engine.process(&mut [0.0; 32], 2);
    while events.pop().is_ok() {}
    commands
        .push(EngineCommand::RemoveAutomationLane {
            track_id: id,
            lane_id,
        })
        .unwrap();
    let mut at = Vec::new();
    for _ in 0..15 {
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 32], 2)),
            (0, 0)
        );
        while let Ok(event) = events.pop() {
            if let EngineEvent::SourceNoteRepeated { position, .. } = event {
                at.push(position.effective_at_samples);
            }
        }
    }
    assert_eq!(at, [84, 128, 200, 256]);
}
