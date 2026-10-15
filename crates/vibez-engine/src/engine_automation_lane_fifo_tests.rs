//! Live lane storage growth preserves current first-match values, channel resolution and later command order.

use super::*;
use crate::clip_insertion::{ClipInsertionOutcome, ClipInsertionTarget, PreparedAutomationLaneInsertion};
use crate::events::AutomationGesturePhase;
use crate::playback_source::PreparedPlaybackSource;
use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
use vibez_core::id::ClipId;
use vibez_core::routing::RoutingChannel;

fn lane(target: AutomationTarget, value: f32) -> AutomationLane {
    let mut lane = AutomationLane::new(target);
    lane.points = vec![
        AutomationPoint { beat: 0.0, value, curve: -0.25 },
        AutomationPoint { beat: 4.0, value, curve: 0.75 },
    ];
    lane
}

fn zero_callback(engine: &mut AudioEngine) {
    assert_eq!(crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(&mut [], 2));
    }), (0, 0));
}

#[test]
fn repeated_small_hints_keep_current_duplicate_lanes_and_live_controls_ahead_of_fifo_stop() {
    const APPENDS: usize = 16;
    let track_id = TrackId::new();
    let first = lane(AutomationTarget::TrackPan, 0.0);
    let original_first = first.clone();
    let original_first_points = first.points.as_ptr();
    let mut duplicate = lane(AutomationTarget::TrackGain, 0.5);
    duplicate.id = first.id;
    let mute = lane(AutomationTarget::TrackMute, 0.0);
    let mut replacement = lane(AutomationTarget::TrackGain, 0.25);
    replacement.id = first.id;
    let mut expected = vec![replacement.clone(), duplicate.clone(), mute.clone()];
    let mut point_owners = vec![replacement.points.as_ptr(), duplicate.points.as_ptr(), mute.points.as_ptr()];
    let audio = Arc::new(DecodedAudio { channels: vec![vec![0.25; 4096]; 2], sample_rate: DEFAULT_SAMPLE_RATE });
    let audio_owner = Arc::downgrade(&audio);
    let clip_id = ClipId::new();
    let clip = EngineClip {
        id: clip_id, audio, position: 0, source_offset: 0, start_marker: 0,
        duration: 4096, loop_enabled: false, loop_start: 0, loop_end: 0,
        linear_gain: 1.0, fades: Default::default(), playback_direction: Default::default(),
        warp_markers: Default::default(),
    };
    let lanes = vec![first, duplicate, mute].into_boxed_slice().into_vec();
    assert_eq!((lanes.len(), lanes.capacity()), (3, 3));
    let mut track = Box::new(EngineTrack::with_playback_source(
        track_id, PreparedPlaybackSource::new(vec![clip], vec![], lanes),
    ));
    track.gain = 1.0;
    track.pan = 0.0;
    let source_owner = std::ptr::from_ref(track.playback_source.as_ref());
    let clips_owner = track.playback_source.clips.as_ptr();
    let channels = [track_id, TrackId::MASTER].map(|id| RoutingChannel {
        id, is_bus: id.is_master(), sends: vec![], effects: vec![],
    });
    let mut routing = crate::routing::PreparedRouting::prepare(&channels, 64).unwrap();
    routing.configure_automation(&[
        (track_id, AutomationTarget::TrackGain), (track_id, AutomationTarget::TrackPan),
        (track_id, AutomationTarget::TrackMute),
    ]).unwrap();
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    commands.push(EngineCommand::AddTrack(track)).unwrap();
    commands.push(EngineCommand::SetTrackGain(TrackId::MASTER, 1.0)).unwrap();
    commands.push(EngineCommand::SetRouting(routing)).unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut output = [0.0; 128];
    let mut capture = [0.0; 128];
    for _ in 0..2 {
        engine.process_block(AudioProcessBlock::new(&mut output, 2));
        while events.pop().is_ok() {}
    }
    assert!(output.as_chunks::<2>().0.iter().all(|frame| *frame == [0.25, 0.0]));
    let start = engine.transport.position();
    let insertion = PreparedAutomationLaneInsertion::new(replacement);
    let replacement_operation = insertion.id();
    commands.push(EngineCommand::SetAutomationLane { track_id, insertion }).unwrap();
    let mut operations = Vec::new();
    for index in 0..APPENDS {
        let incoming = if index == 0 {
            AutomationLane::new(AutomationTarget::TrackPan)
        } else {
            let (target, value) = match index % 3 {
                0 => (AutomationTarget::TrackGain, 0.875),
                1 => (AutomationTarget::TrackPan, 1.0),
                _ => (AutomationTarget::TrackMute, 1.0),
            };
            lane(target, value)
        };
        expected.push(incoming.clone());
        point_owners.push(incoming.points.as_ptr());
        let insertion = PreparedAutomationLaneInsertion::new(incoming);
        operations.push(insertion.id());
        commands.push(EngineCommand::SetAutomationLane { track_id, insertion }).unwrap();
    }
    commands.push(EngineCommand::RemoveAutomationLane { track_id, lane_id: expected[0].id }).unwrap();
    commands.push(EngineCommand::EndAutomationGesture { track_id, target: AutomationTarget::TrackGain }).unwrap();
    commands.push(EngineCommand::Stop).unwrap();
    let mut completed = Vec::new();
    let mut retired = Vec::new();
    let mut appended = 0;
    let mut boundaries = Vec::new();
    let mut public_order = Vec::new();
    let assert_current = |engine: &AudioEngine, appended: usize, removed: bool| {
        let source = &engine.tracks[0].playback_source;
        assert!(std::ptr::eq(source.as_ref(), source_owner));
        assert_eq!(source.clips.as_ptr(), clips_owner);
        assert_eq!(source.clips.len(), 1);
        assert_eq!((source.clips[0].id, source.clips[0].position, source.clips[0].duration), (clip_id, 0, 4096));
        assert_eq!(audio_owner.strong_count(), 1);
        let begin = usize::from(removed);
        assert_eq!(source.automation, expected[begin..3 + appended]);
        for (value, owner) in source.automation.iter().zip(&point_owners[begin..3 + appended]) {
            assert_eq!(value.points.as_ptr(), *owner);
        }
        assert_eq!(engine.transport.audio_length(), Some(4096));
    };
    for _ in 0..=APPENDS {
        let clocks = (engine.transport.position(), engine.performance_position, engine.output_position);
        zero_callback(&mut engine);
        assert_eq!((engine.transport.position(), engine.performance_position, engine.output_position), clocks);
        let mut pending = None;
        while let Ok(event) = events.pop() {
            match event {
                EngineEvent::RetiredAutomationLane(value) => {
                    if retired.is_empty() {
                        assert_eq!(value, original_first);
                        assert_eq!(value.points.as_ptr(), original_first_points);
                        assert!(completed.is_empty());
                    } else {
                        assert_eq!(retired.len(), 1);
                        assert_eq!(appended, APPENDS);
                        assert_eq!(value, expected[0]);
                        assert_eq!(value.points.as_ptr(), point_owners[0]);
                    }
                    retired.push(value);
                }
                EngineEvent::ClipInsertionCompleted(value) => {
                    assert_eq!(value.outcome, ClipInsertionOutcome::Applied);
                    assert_eq!(value.track_id, track_id);
                    if completed.is_empty() {
                        assert_eq!(retired.len(), 1);
                        assert_eq!(value.id(), replacement_operation);
                        assert_eq!(value.target, ClipInsertionTarget::AutomationLane(expected[0].id));
                    } else {
                        assert_eq!(value.id(), operations[appended]);
                        assert_eq!(value.target, ClipInsertionTarget::AutomationLane(expected[3 + appended].id));
                        appended += 1;
                    }
                    completed.push(value);
                }
                EngineEvent::ClipInsertionStorageRequested(request) => {
                    assert_eq!(request.id, operations[appended]);
                    assert_eq!((request.track_id, request.target), (track_id, ClipInsertionTarget::AutomationLane(expected[3 + appended].id)));
                    assert!(pending.replace(request).is_none());
                }
                EngineEvent::AutomationGestureChanged { track_id: id, target, normalized_value, phase, effective_at_samples } => {
                    assert_eq!((id, target, normalized_value, phase, effective_at_samples), (track_id, AutomationTarget::TrackGain, 0.5, AutomationGesturePhase::End, clocks.0));
                    assert_eq!((appended, retired.len()), (APPENDS, 2));
                    public_order.push("gesture-end");
                }
                EngineEvent::PlaybackStopped => {
                    assert_eq!(public_order, ["gesture-end"]);
                    public_order.push("stop");
                }
                EngineEvent::PlaybackPosition(position) => assert_eq!(position, clocks.0),
                EngineEvent::TrackMeter { track_id: id, peak_l, peak_r } => {
                    assert!([track_id, TrackId::MASTER].contains(&id));
                    assert_eq!((peak_l, peak_r), (0.0, 0.0));
                }
                EngineEvent::Metering { peak_l, peak_r, rms_l, rms_r } => assert_eq!((peak_l, peak_r, rms_l, rms_r), (0.0, 0.0, 0.0, 0.0)),
                event => panic!("unexpected live lane FIFO event: {event:?}"),
            }
        }
        assert_current(&engine, appended, retired.len() == 2);
        let Some(request) = pending else { break };
        assert!(engine.pending_clip_insertion.is_some());
        assert!(engine.transport.is_playing());
        assert!(public_order.is_empty());
        assert_eq!(retired.len(), 1);
        let source = &engine.tracks[0].playback_source;
        assert_eq!(source.automation.len(), source.automation.capacity());
        boundaries.push(source.automation.len());
        let slots = commands.slots();
        assert_eq!(crate::retirement::tests::allocations(|| {
            engine.process_block(AudioProcessBlock::new(&mut output, 2).with_track_output_capture(track_id.raw(), &mut capture));
        }), (0, 0));
        assert_eq!((engine.transport.position(), engine.performance_position, engine.output_position), (clocks.0 + 64, clocks.1 + 64, clocks.2 + 64));
        assert_eq!(commands.slots(), slots);
        assert!(engine.transport.is_playing());
        assert_current(&engine, appended, false);
        assert!(output.as_chunks::<2>().0.iter().chain(capture.as_chunks::<2>().0).all(|frame| *frame == [0.125, 0.0]));
        while let Ok(event) = events.pop() {
            match event {
                EngineEvent::PlaybackPosition(position) => assert_eq!(position, clocks.0 + 64),
                EngineEvent::TrackMeter { track_id: id, peak_l, peak_r } => {
                    assert!([track_id, TrackId::MASTER].contains(&id));
                    assert_eq!((peak_l, peak_r), (0.125, 0.0));
                }
                EngineEvent::Metering { peak_l, peak_r, rms_l, rms_r } => assert_eq!((peak_l, peak_r, rms_l, rms_r), (0.125, 0.0, 0.125, 0.0)),
                event => panic!("a blocked lane edit must not admit a later FIFO command or owner: {event:?}"),
            }
        }
        request.fulfill();
    }
    assert!(boundaries.len() >= 2, "the same default hint must cross multiple actual full capacities");
    assert!(boundaries.windows(2).all(|pair| pair[1] > pair[0]));
    assert_eq!(appended, APPENDS);
    assert_eq!(completed.len(), APPENDS + 1);
    assert_eq!(retired.len(), 2);
    assert_eq!(public_order, ["gesture-end", "stop"]);
    assert!(engine.pending_clip_insertion.is_none());
    assert!(!engine.transport.is_playing());
    assert_eq!(commands.slots(), vibez_core::constants::RING_BUFFER_CAPACITY);
    assert_eq!(engine.transport.position(), start + boundaries.len() as u64 * 64);
    eprintln!("lane FIFO: appends={appended}; actual full boundaries={boundaries:?}; cursor={}; current points and first-ID removal preserved", engine.transport.position());
}

#[test]
fn lane_growth_and_empty_first_id_replacement_resolve_master_first_track_then_bus() {
    for selected in [0usize, 1, 6] {
        let track = TrackId::new();
        let bus = TrackId::new();
        let prototype = lane(AutomationTarget::TrackGain, 0.5);
        let make_channel = |id| Box::new(EngineTrack::with_playback_source(id,
            PreparedPlaybackSource::new(vec![], vec![], vec![prototype.clone()].into_boxed_slice().into_vec())));
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        for id in [track, track, TrackId::MASTER] {
            commands.push(EngineCommand::AddTrack(make_channel(id))).unwrap();
        }
        for id in [track, TrackId::MASTER, bus, bus] {
            commands.push(EngineCommand::AddBus(make_channel(id))).unwrap();
        }
        commands.push(EngineCommand::SetAutomationLane {
            track_id: TrackId::MASTER, insertion: PreparedAutomationLaneInsertion::new(prototype.clone()),
        }).unwrap();
        zero_callback(&mut engine);
        while events.pop().is_ok() {}
        let target = match selected { 0 => TrackId::MASTER, 1 => track, _ => bus };
        let sources = |engine: &AudioEngine| {
            std::iter::once(engine.master.playback_source.as_ref())
                .chain(engine.tracks.iter().map(|channel| channel.playback_source.as_ref()))
                .chain(engine.buses.iter().map(|channel| channel.playback_source.as_ref()))
                .map(|source| (std::ptr::from_ref(source), source.automation.clone(), source.automation[0].points.as_ptr()))
                .collect::<Vec<_>>()
        };
        let original = sources(&engine);
        assert_eq!(original.len(), 8);
        let incoming = lane(AutomationTarget::TrackPan, 0.75);
        let expected = incoming.clone();
        let points = incoming.points.as_ptr();
        let insertion = PreparedAutomationLaneInsertion::with_capacity(incoming, 0);
        let operation = insertion.id();
        commands.push(EngineCommand::SetAutomationLane { track_id: target, insertion }).unwrap();
        zero_callback(&mut engine);
        assert_eq!(sources(&engine), original);
        let EngineEvent::ClipInsertionStorageRequested(request) = events.pop().unwrap() else {
            panic!("the selected current channel must request actual full outer storage");
        };
        assert_eq!(request.id, operation);
        assert_eq!((request.track_id, request.target), (target, ClipInsertionTarget::AutomationLane(expected.id)));
        let assert_telemetry = |events: &mut Consumer<EngineEvent>| {
            while let Ok(event) = events.pop() {
                match event {
                    EngineEvent::PlaybackPosition(0) => {}
                    EngineEvent::TrackMeter { track_id: id, peak_l, peak_r } => {
                        assert!([track, TrackId::MASTER, bus].contains(&id));
                        assert_eq!((peak_l, peak_r), (0.0, 0.0));
                    }
                    EngineEvent::Metering { peak_l, peak_r, rms_l, rms_r } => assert_eq!((peak_l, peak_r, rms_l, rms_r), (0.0, 0.0, 0.0, 0.0)),
                    event => panic!("unexpected additional channel edit or owner: {event:?}"),
                }
            }
        };
        assert_telemetry(&mut events);
        request.fulfill();
        zero_callback(&mut engine);
        let EngineEvent::ClipInsertionCompleted(completed) = events.pop().unwrap() else { panic!("selected channel append must complete once") };
        assert_eq!(completed.id(), operation);
        assert_eq!(completed.outcome, ClipInsertionOutcome::Applied);
        assert_eq!((completed.track_id, completed.target), (target, ClipInsertionTarget::AutomationLane(expected.id)));
        let current = sources(&engine);
        for (index, (pointer, lanes, first_points)) in current.iter().enumerate() {
            assert_eq!(*pointer, original[index].0);
            assert_eq!(*first_points, original[index].2);
            if index == selected { assert_eq!(lanes, &[prototype.clone(), expected.clone()]); }
            else { assert_eq!(lanes, &original[index].1); }
        }
        let channel = engine.channel_mut(target).unwrap();
        assert_eq!(channel.playback_source.automation[1].points.as_ptr(), points);
        assert_telemetry(&mut events);
        let mut empty = AutomationLane::new(AutomationTarget::TrackMute);
        empty.id = prototype.id;
        let expected_empty = empty.clone();
        let insertion = PreparedAutomationLaneInsertion::with_capacity(empty, 0);
        let replacement_operation = insertion.id();
        commands.push(EngineCommand::SetAutomationLane { track_id: target, insertion }).unwrap();
        zero_callback(&mut engine);
        let EngineEvent::RetiredAutomationLane(retired) = events.pop().unwrap() else { panic!("replacement keeps the original first lane emission") };
        assert_eq!(retired, prototype);
        assert_eq!(retired.points.as_ptr(), original[selected].2);
        let EngineEvent::ClipInsertionCompleted(replaced) = events.pop().unwrap() else { panic!("empty replacement returns its complete control") };
        assert_eq!(replaced.id(), replacement_operation);
        assert_eq!(replaced.outcome, ClipInsertionOutcome::Applied);
        let current = sources(&engine);
        for (index, (pointer, lanes, first_points)) in current.iter().enumerate() {
            assert_eq!(*pointer, original[index].0);
            if index == selected { assert_eq!(lanes, &[expected_empty.clone(), expected.clone()]); }
            else { assert_eq!((lanes, first_points), (&original[index].1, &original[index].2)); }
        }
        assert_telemetry(&mut events);
        assert!(engine.pending_clip_insertion.is_none());
        assert_eq!(commands.slots(), vibez_core::constants::RING_BUFFER_CAPACITY);
        assert_eq!((engine.transport.position(), engine.performance_position, engine.output_position), (0, 0, 0));
        assert_eq!(engine.transport.audio_length(), None);
        drop((completed, replaced, retired));
    }
}
