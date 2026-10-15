//! Lane admission and completion retain complete owners under bounded retirement pressure.

use super::*;
use std::sync::Weak;
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::routing::RoutingChannel;

struct Fixture {
    engine: AudioEngine,
    commands: Producer<EngineCommand>,
    events: Consumer<EngineEvent>,
    track: TrackId,
    resident: AutomationLane,
    points: *const AutomationPoint,
    outer: *const AutomationLane,
    source: *const PreparedPlaybackSource,
    clips: *const EngineClip,
    audio: Weak<DecodedAudio>,
}

impl Fixture {
    fn new() -> Self {
        let track = TrackId::new();
        let resident = lane(AutomationTarget::TrackGain, 0.5);
        let points = resident.points.as_ptr();
        let expected = resident.clone();
        let lanes = vec![resident].into_boxed_slice().into_vec();
        assert_eq!((lanes.len(), lanes.capacity()), (1, 1));
        let outer = lanes.as_ptr();
        let audio = Arc::new(DecodedAudio {
            channels: vec![vec![0.25; 4096]; 2], sample_rate: 44_100,
        });
        let audio_owner = Arc::downgrade(&audio);
        let clip = EngineClip {
            id: ClipId::new(), audio, position: 0, source_offset: 0, start_marker: 0,
            duration: 4096, loop_enabled: false, loop_start: 0, loop_end: 0,
            linear_gain: 1.0, fades: Default::default(), playback_direction: Default::default(),
            warp_markers: Default::default(),
        };
        let mut channel = Box::new(EngineTrack::with_playback_source(track,
            PreparedPlaybackSource::new(vec![clip], vec![], lanes)));
        channel.gain = 1.0;
        channel.pan = 0.0;
        let source = std::ptr::from_ref(channel.playback_source.as_ref());
        let clips = channel.playback_source.clips.as_ptr();
        let model = [track, TrackId::MASTER].map(|id| RoutingChannel {
            id, is_bus: id.is_master(), sends: vec![], effects: vec![],
        });
        let mut routing = crate::routing::PreparedRouting::prepare(&model, 64).unwrap();
        routing.configure_automation(&[(track, AutomationTarget::TrackGain),
            (track, AutomationTarget::TrackPan)]).unwrap();
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        commands.push(EngineCommand::AddTrack(channel)).unwrap();
        commands.push(EngineCommand::SetTrackGain(TrackId::MASTER, 1.0)).unwrap();
        commands.push(EngineCommand::SetRouting(routing)).unwrap();
        commands.push(EngineCommand::Play).unwrap();
        let mut output = [0.0; 128];
        for _ in 0..2 {
            engine.process_block(AudioProcessBlock::new(&mut output, 2));
            while events.pop().is_ok() {}
        }
        assert!(output.as_chunks::<2>().0.iter().all(|frame| *frame == [0.25, 0.0]));
        Self { engine, commands, events, track, resident: expected, points, outer,
            source, clips, audio: audio_owner }
    }

    fn callback(&mut self, sounding: bool) {
        let position = self.engine.transport().position();
        let output_position = self.engine.output_position_samples();
        let mut output = [0.0; 128];
        let mut capture = [0.0; 128];
        let length = if sounding { output.len() } else { 0 };
        assert_eq!(crate::retirement::tests::allocations(|| {
            self.engine.process_block(AudioProcessBlock::new(&mut output[..length], 2)
                .with_track_output_capture(self.track.raw(), &mut capture[..length]));
        }), (0, 0));
        let frames = u64::from(sounding) * 64;
        assert_eq!(self.engine.transport().position(), position + frames);
        assert_eq!(self.engine.output_position_samples(), output_position + frames);
        if sounding {
            assert!(self.engine.transport().is_playing());
            for frame in output.as_chunks::<2>().0.iter().chain(capture.as_chunks::<2>().0) {
                assert_eq!(*frame, [0.25, 0.0]);
            }
        }
    }

    fn assert_current(&self, expected: &[AutomationLane], points: &[*const AutomationPoint], outer: Option<*const AutomationLane>) {
        let source = &self.engine.tracks()[0].playback_source;
        assert!(std::ptr::eq(source.as_ref(), self.source));
        assert_eq!(source.clips.as_ptr(), self.clips);
        assert_eq!(source.clips.len(), 1);
        let clip = &source.clips[0];
        assert_eq!((clip.position, clip.source_offset, clip.start_marker, clip.duration), (0, 0, 0, 4096));
        assert_eq!((clip.loop_enabled, clip.loop_start, clip.loop_end, clip.linear_gain), (false, 0, 0, 1.0));
        assert_eq!(self.audio.strong_count(), 1);
        assert_eq!(&source.automation, expected);
        for (value, pointer) in source.automation.iter().zip(points) {
            assert_eq!(value.points.as_ptr(), *pointer);
        }
        assert_eq!(source.automation.len(), points.len());
        if let Some(outer) = outer { assert_eq!(source.automation.as_ptr(), outer); }
        assert_eq!(self.engine.transport().audio_length(), Some(4096));
        assert_eq!(self.engine.transport().bpm(), 120.0);
    }

    fn assert_original(&self) {
        self.assert_current(std::slice::from_ref(&self.resident), &[self.points], Some(self.outer));
        assert_eq!(self.engine.tracks()[0].playback_source.automation.capacity(), 1);
    }

    fn finish_telemetry(&mut self) {
        let mut stopped = 0;
        let mut position = 0;
        let mut meters = 0;
        while let Ok(event) = self.events.pop() {
            match event {
                EngineEvent::PlaybackStopped => stopped += 1,
                EngineEvent::PlaybackPosition(value) => {
                    assert_eq!(value, self.engine.transport().position());
                    position += 1;
                }
                EngineEvent::TrackMeter { track_id, peak_l, peak_r } => {
                    assert!([self.track, TrackId::MASTER].contains(&track_id));
                    assert_eq!((peak_l, peak_r), (0.0, 0.0));
                }
                EngineEvent::Metering { peak_l, peak_r, rms_l, rms_r } => {
                    assert_eq!((peak_l, peak_r, rms_l, rms_r), (0.0, 0.0, 0.0, 0.0));
                    meters += 1;
                }
                event => panic!("unexpected additional owner or pressure event: {event:?}"),
            }
        }
        assert_eq!((stopped, position, meters), (1, 1, 1));
        assert!(!self.engine.transport().is_playing());
        assert_eq!(self.commands.slots(), vibez_core::constants::RING_BUFFER_CAPACITY);
    }
}

struct Delta {
    id: ClipInsertionId,
    identity: Weak<()>,
    control: *const PreparedClipInsertion,
    ring: *const RingBuffer<ClipInsertionStorage>,
    eager: *const AutomationLane,
    incoming: AutomationLane,
    points: *const AutomationPoint,
    target: TrackId,
}

impl Delta {
    fn prepare(incoming: AutomationLane, target: TrackId) -> (Self, EngineCommand) {
        let expected = incoming.clone();
        let points = incoming.points.as_ptr();
        let insertion = PreparedAutomationLaneInsertion::with_capacity(incoming, 0);
        let id = insertion.id();
        let identity = Arc::downgrade(&id.0);
        let control = std::ptr::from_ref(insertion.0.as_ref());
        let ring = std::ptr::from_ref(insertion.0.receiver.buffer());
        let ClipInsertionStorage::AutomationLane(eager) = &insertion.0.storage else { unreachable!() };
        let eager = eager.as_ptr();
        (Self { id, identity, control, ring, eager, incoming: expected, points, target },
            EngineCommand::SetAutomationLane { track_id: target, insertion })
    }

    fn assert_control(&self, completed: &CompletedClipInsertion, outcome: ClipInsertionOutcome, waited: bool) {
        assert_eq!(completed.id(), self.id);
        assert_eq!((completed.track_id, completed.target), (self.target, ClipInsertionTarget::AutomationLane(self.incoming.id)));
        assert_eq!(completed.outcome, outcome);
        assert_eq!(std::ptr::from_ref(completed.owner.as_ref()), self.control);
        assert_eq!(std::ptr::from_ref(completed.owner.receiver.buffer()), self.ring);
        assert_eq!(completed.owner.sender.is_none(), waited);
        assert_eq!(completed.owner.receiver.is_abandoned(), waited);
        let ClipInsertionStorage::AutomationLane(eager) = &completed.owner.storage else { unreachable!() };
        assert_eq!(eager.as_ptr(), self.eager);
        assert_eq!((eager.len(), eager.capacity()), (0, 0));
    }
}

fn stage_pressure(fixture: &mut Fixture) -> Vec<(AutomationLane, *const AutomationPoint)> {
    let mut fillers = 0;
    while fixture.engine.event_tx.push(EngineEvent::PlaybackPosition(u64::MAX)).is_ok() { fillers += 1; }
    assert_eq!(vibez_core::constants::RING_BUFFER_CAPACITY, 1024);
    assert_eq!(fillers, 1024);
    assert_eq!(fixture.engine.event_tx.slots(), 0);
    assert!(fixture.engine.pending_retirements.is_empty());
    assert_eq!(fixture.engine.pending_retirements.capacity(), 32);
    let mut expected = Vec::new();
    for index in 0..32 {
        let value = lane(AutomationTarget::TrackPan, index as f32 / 64.0);
        expected.push((value.clone(), value.points.as_ptr()));
        // Off-callback fixture staging supplies ordinary owners; public commands,
        // replies and the actual full UI ring still exercise admission and return.
        fixture.engine.retire_event(EngineEvent::RetiredAutomationLane(value));
    }
    assert_eq!(fixture.engine.pending_retirements.len(), 32);
    expected
}

fn pop_filler(events: &mut Consumer<EngineEvent>) {
    assert!(matches!(events.pop().unwrap(), EngineEvent::PlaybackPosition(u64::MAX)));
}

fn take_lane(events: &mut Consumer<EngineEvent>, expected: &(AutomationLane, *const AutomationPoint)) -> AutomationLane {
    let EngineEvent::RetiredAutomationLane(value) = events.pop().unwrap() else { panic!("ordinary owners must retain their exact LIFO publication position") };
    assert_eq!(&value, &expected.0);
    assert_eq!(value.points.as_ptr(), expected.1);
    value
}

fn take_control(events: &mut Consumer<EngineEvent>) -> CompletedClipInsertion {
    let EngineEvent::ClipInsertionCompleted(value) = events.pop().unwrap() else { panic!("the complete control must be returned at its LIFO publication position") };
    value
}

#[test]
fn initial_replacement_and_missing_lane_require_two_current_credits_before_pop() {
    for missing in [false, true] {
        let mut fixture = Fixture::new();
        let mut incoming = lane(AutomationTarget::TrackGain, 0.5);
        if !missing { incoming.id = fixture.resident.id; }
        incoming.points[0].curve = 0.25;
        let target = if missing { TrackId::new() } else { fixture.track };
        let (delta, command) = Delta::prepare(incoming, target);
        let expected = stage_pressure(&mut fixture);
        let allocation = fixture.engine.pending_retirements.as_ptr();
        fixture.commands.push(command).unwrap();
        fixture.commands.push(EngineCommand::Stop).unwrap();
        let slots = fixture.commands.slots();
        fixture.callback(true);
        fixture.assert_original();
        assert_eq!(fixture.commands.slots(), slots);
        assert_eq!(fixture.engine.pending_retirements.len(), 32);
        assert_eq!(delta.identity.strong_count(), 2);
        pop_filler(&mut fixture.events);
        fixture.callback(false);
        fixture.assert_original();
        assert_eq!(fixture.commands.slots(), slots);
        assert_eq!(fixture.engine.pending_retirements.len(), 31);
        pop_filler(&mut fixture.events);
        fixture.callback(false);
        assert_eq!(fixture.commands.slots(), slots + 1);
        assert_eq!(fixture.engine.pending_retirements.len(), 32);
        assert_eq!(fixture.engine.pending_retirements.as_ptr(), allocation);
        assert_eq!(fixture.engine.pending_retirements.capacity(), 32);
        let expected_retired = if missing { (delta.incoming.clone(), delta.points) } else { (fixture.resident.clone(), fixture.points) };
        let EngineEvent::RetiredAutomationLane(value) = &fixture.engine.pending_retirements[30] else { panic!("lane return must precede its control") };
        assert_eq!(value, &expected_retired.0);
        assert_eq!(value.points.as_ptr(), expected_retired.1);
        let EngineEvent::ClipInsertionCompleted(value) = &fixture.engine.pending_retirements[31] else { panic!("both initial credits must retain complete owners") };
        let outcome = if missing { ClipInsertionOutcome::MissingChannel } else { ClipInsertionOutcome::Applied };
        delta.assert_control(value, outcome, false);
        assert!(value.owner.clip.is_none());
        assert!(value.owner.reply.is_none());
        if missing { fixture.assert_original(); }
        else { fixture.assert_current(std::slice::from_ref(&delta.incoming), &[delta.points], Some(fixture.outer)); }
        fixture.callback(true);
        assert_eq!(fixture.commands.slots(), slots + 1);
        assert_eq!(fixture.engine.pending_retirements.len(), 32);
        assert!(fixture.engine.transport().is_playing());
        for _ in 0..1022 { pop_filler(&mut fixture.events); }
        let mut returned = vec![take_lane(&mut fixture.events, &expected[31]), take_lane(&mut fixture.events, &expected[30])];
        assert!(fixture.events.is_empty());
        fixture.callback(false);
        let completed = take_control(&mut fixture.events);
        delta.assert_control(&completed, outcome, false);
        let retired = take_lane(&mut fixture.events, &(expected_retired.0.clone(), expected_retired.1));
        for value in expected[..30].iter().rev() { returned.push(take_lane(&mut fixture.events, value)); }
        assert_eq!(returned.len(), 32);
        fixture.finish_telemetry();
        assert!(fixture.engine.pending_retirements.is_empty());
        assert_eq!(fixture.engine.pending_retirements.as_ptr(), allocation);
        assert_eq!(fixture.engine.pending_retirements.capacity(), 32);
        if missing { fixture.assert_original(); }
        else { fixture.assert_current(std::slice::from_ref(&delta.incoming), &[delta.points], Some(fixture.outer)); }
        drop((returned, retired, completed));
        assert_eq!(delta.identity.strong_count(), 1);
    }
}

#[test]
fn pending_ready_or_abandoned_lane_uses_one_current_credit_and_keeps_stop_waiting() {
    for outcome in [ClipInsertionOutcome::Applied, ClipInsertionOutcome::Abandoned] {
        let mut fixture = Fixture::new();
        let (delta, command) = Delta::prepare(lane(AutomationTarget::TrackPan, 0.0), fixture.track);
        fixture.commands.push(command).unwrap();
        fixture.commands.push(EngineCommand::Stop).unwrap();
        let slots = fixture.commands.slots();
        fixture.callback(false);
        fixture.assert_original();
        assert_eq!(fixture.commands.slots(), slots + 1);
        assert!(fixture.engine.pending_retirements.is_empty());
        let EngineEvent::ClipInsertionStorageRequested(request) = fixture.events.pop().unwrap() else { panic!("full current lane storage must request once") };
        assert_eq!(request.id, delta.id);
        assert_eq!((request.track_id, request.target), (fixture.track, ClipInsertionTarget::AutomationLane(delta.incoming.id)));
        assert_eq!(std::ptr::from_ref(request.sender.buffer()), delta.ring);
        assert_eq!(request.capacity, 4);
        while let Ok(event) = fixture.events.pop() {
            match event {
                EngineEvent::PlaybackPosition(value) => assert_eq!(value, fixture.engine.transport().position()),
                EngineEvent::Metering { peak_l, peak_r, rms_l, rms_r } => assert_eq!((peak_l, peak_r, rms_l, rms_r), (0.0, 0.0, 0.0, 0.0)),
                event => panic!("unexpected request-boundary event: {event:?}"),
            }
        }
        if outcome == ClipInsertionOutcome::Applied { request.fulfill(); } else { drop(request); }
        let expected = stage_pressure(&mut fixture);
        let allocation = fixture.engine.pending_retirements.as_ptr();
        fixture.callback(true);
        fixture.assert_original();
        assert_eq!(fixture.commands.slots(), slots + 1);
        assert_eq!(fixture.engine.pending_retirements.len(), 32);
        pop_filler(&mut fixture.events);
        fixture.callback(false);
        assert_eq!(fixture.commands.slots(), slots + 1);
        assert_eq!(fixture.engine.pending_retirements.len(), 32);
        assert_eq!(fixture.engine.pending_retirements.as_ptr(), allocation);
        let EngineEvent::ClipInsertionCompleted(value) = &fixture.engine.pending_retirements[31] else { panic!("one available credit must retain the complete control") };
        delta.assert_control(value, outcome, true);
        if outcome == ClipInsertionOutcome::Applied {
            let Some(ClipInsertionStorage::AutomationLane(old)) = &value.owner.reply else { panic!("the complete control retains the original outer allocation") };
            assert_eq!(old.as_ptr(), fixture.outer);
            assert_eq!((old.len(), old.capacity()), (0, 1));
            assert!(value.owner.clip.is_none());
            fixture.assert_current(&[fixture.resident.clone(), delta.incoming.clone()], &[fixture.points, delta.points], None);
            let current = &fixture.engine.tracks()[0].playback_source.automation;
            assert_ne!(current.as_ptr(), fixture.outer);
            assert_eq!((current.len(), current.capacity()), (2, 4));
        } else {
            assert!(value.owner.reply.is_none());
            let Some(ClipInsertionDelta::AutomationLane(original)) = &value.owner.clip else { panic!("abandonment retains its original whole lane") };
            assert_eq!(original, &delta.incoming);
            assert_eq!(original.points.as_ptr(), delta.points);
            fixture.assert_original();
        }
        fixture.callback(true);
        assert_eq!(fixture.commands.slots(), slots + 1);
        assert_eq!(fixture.engine.pending_retirements.len(), 32);
        assert!(fixture.engine.transport().is_playing());
        for _ in 0..1023 { pop_filler(&mut fixture.events); }
        let mut returned = vec![take_lane(&mut fixture.events, &expected[31])];
        assert!(fixture.events.is_empty());
        fixture.callback(false);
        let completed = take_control(&mut fixture.events);
        delta.assert_control(&completed, outcome, true);
        for value in expected[..31].iter().rev() { returned.push(take_lane(&mut fixture.events, value)); }
        assert_eq!(returned.len(), 32);
        fixture.finish_telemetry();
        assert!(fixture.engine.pending_retirements.is_empty());
        assert_eq!(fixture.engine.pending_retirements.as_ptr(), allocation);
        assert_eq!(fixture.engine.pending_retirements.capacity(), 32);
        if outcome == ClipInsertionOutcome::Applied {
            fixture.assert_current(&[fixture.resident.clone(), delta.incoming.clone()], &[fixture.points, delta.points], None);
            let current = &fixture.engine.tracks()[0].playback_source.automation;
            assert_ne!(current.as_ptr(), fixture.outer);
            assert_eq!((current.len(), current.capacity()), (2, 4));
        } else { fixture.assert_original(); }
        drop((returned, completed));
        assert_eq!(delta.identity.strong_count(), 1);
    }
}

#[path = "automation_lane_insertion_lifecycle_tests.rs"]
mod lifecycle_tests;
