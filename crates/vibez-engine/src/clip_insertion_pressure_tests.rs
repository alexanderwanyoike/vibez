//! Real AddClip storage exchanges retain complete owners under injected bounded pressure.

use super::*;
use crate::commands::EngineCommand;
use crate::engine::{AudioEngine, AudioProcessBlock, DEFAULT_SAMPLE_RATE};
use crate::events::EngineEvent;
use crate::mixer::EngineTrack;
use std::sync::Weak;
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
use vibez_core::routing::RoutingChannel;
use vibez_core::warp_marker::WarpMarkers;

struct Fixture {
    engine: AudioEngine,
    commands: Producer<EngineCommand>,
    events: Consumer<EngineEvent>,
    track: TrackId,
    resident: Weak<DecodedAudio>,
    live_allocation: *const EngineClip,
    staging_allocation: *const EngineClip,
}

fn setup() -> Fixture {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let audio = Arc::new(DecodedAudio {
        channels: vec![vec![0.25; 4096]],
        sample_rate: DEFAULT_SAMPLE_RATE,
    });
    let resident = Arc::downgrade(&audio);
    let source = PreparedPlaybackSource::new(vec![EngineClip {
        id: ClipId::new(), audio, position: 0, source_offset: 0,
        start_marker: 0, duration: 4096, loop_enabled: false,
        loop_start: 0, loop_end: 0, linear_gain: 1.0,
        fades: Default::default(), playback_direction: Default::default(),
        warp_markers: Default::default(),
    }], Vec::new(), Vec::new());
    let mut channel = EngineTrack::with_playback_source(track, source);
    channel.pan = 0.0;
    commands.push(EngineCommand::AddTrack(Box::new(channel))).unwrap();
    let model: Vec<_> = [track, TrackId::MASTER].into_iter().map(|id| RoutingChannel {
        id, is_bus: id.is_master(), sends: Vec::new(), effects: Vec::new(),
    }).collect();
    commands.push(EngineCommand::SetRouting(
        crate::routing::PreparedRouting::prepare(&model, 8).unwrap(),
    )).unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut warmup = [0.0; 16];
    engine.process_block(AudioProcessBlock::new(&mut warmup, 2));
    while events.pop().is_ok() {}
    assert_signal(&warmup, 0.25);
    let source = &engine.tracks()[0].playback_source;
    assert_eq!(source.clips.len(), source.clips.capacity());
    let live_allocation = source.clips.as_ptr();
    let staging_allocation = source.removed_audio_clips.as_ptr();
    Fixture { engine, commands, events, track, resident, live_allocation, staging_allocation }
}

fn callback(engine: &mut AudioEngine, sounding: bool) -> [f32; 16] {
    let mut output = [0.0; 16];
    let length = if sounding { output.len() } else { 0 };
    assert_eq!(crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(&mut output[..length], 2));
    }), (0, 0));
    output
}

fn assert_signal(output: &[f32], value: f32) {
    for &[left, right] in output.as_chunks::<2>().0 {
        assert!((left - value).abs() < 1e-7, "expected {value}, actual {left}");
        assert_eq!(right, 0.0);
    }
}

struct Delta {
    command: Option<EngineCommand>,
    id: ClipInsertionId,
    clip: ClipId,
    audio: Weak<DecodedAudio>,
    control: *const PreparedClipInsertion,
    eager: *const EngineClip,
    eager_staging: *const EngineClip,
    markers: *const vibez_core::warp_marker::WarpMarker,
    expected_markers: WarpMarkers,
}

fn delta(track: TrackId) -> Delta {
    let insertion = PreparedClipInsertion::new();
    let id = insertion.id();
    let control = std::ptr::from_ref(insertion.as_ref());
    let eager = insertion.storage.clips.as_ptr();
    let eager_staging = insertion.storage.removed_audio_clips.as_ptr();
    let clip = ClipId::new();
    let audio = Arc::new(DecodedAudio {
        channels: vec![vec![0.0625; 4096]], sample_rate: DEFAULT_SAMPLE_RATE,
    });
    let weak = Arc::downgrade(&audio);
    let mut warp_markers = WarpMarkers::default();
    assert!(warp_markers.add(32, 64, 0, 128, 128));
    let markers = warp_markers.as_slice().as_ptr();
    let expected_markers = warp_markers.clone();
    let command = EngineCommand::AddClip {
        insertion, track_id: track, clip_id: clip, audio, position: 0,
        source_offset: 0, start_marker: 0, duration: 4096, loop_enabled: true,
        loop_start: 0, loop_end: 128, linear_gain: 1.0,
        fades: Default::default(), playback_direction: Default::default(), warp_markers,
    };
    Delta { command: Some(command), id, clip, audio: weak, control, eager,
        eager_staging, markers, expected_markers }
}

fn full_ui_ring(engine: &mut AudioEngine) -> usize {
    let mut count = 0;
    while engine.event_tx.push(EngineEvent::PlaybackPosition(u64::MAX)).is_ok() {
        count += 1;
    }
    assert!(count > 0);
    assert_eq!(engine.event_tx.slots(), 0);
    count
}

fn inject_full_retirement_store(engine: &mut AudioEngine) -> Vec<(AutomationLane, *const AutomationPoint)> {
    assert!(engine.pending_retirements.is_empty());
    assert_eq!(engine.pending_retirements.capacity(), 32);
    let mut expected = Vec::new();
    for index in 0..engine.pending_retirements.capacity() {
        let mut lane = AutomationLane::new(AutomationTarget::TrackPan);
        lane.points = vec![AutomationPoint { beat: 0.0, value: index as f32 / 64.0, curve: 0.0 }];
        let allocation = lane.points.as_ptr();
        expected.push((lane.clone(), allocation));
        // These are explicit pressure-fixture owners. Injection is off callback;
        // every AddClip drain, wait and completion below uses the public engine.
        engine.retire_event(EngineEvent::RetiredAutomationLane(lane));
    }
    assert_eq!(engine.pending_retirements.len(), 32);
    expected
}

#[derive(Default)]
struct Received {
    fillers: usize,
    lanes: Vec<AutomationLane>,
    requests: Vec<ClipInsertionStorageRequest>,
    completed: Vec<CompletedClipInsertion>,
}

impl Received {
    fn drain(&mut self, events: &mut Consumer<EngineEvent>) {
        while let Ok(event) = events.pop() {
            match event {
                EngineEvent::PlaybackPosition(u64::MAX) => self.fillers += 1,
                EngineEvent::RetiredAutomationLane(lane) => self.lanes.push(lane),
                EngineEvent::ClipInsertionStorageRequested(request) => self.requests.push(request),
                EngineEvent::ClipInsertionCompleted(completed) => self.completed.push(completed),
                _ => {}
            }
        }
    }

    fn assert_lanes(&self, expected: &[(AutomationLane, *const AutomationPoint)]) {
        assert_eq!(self.lanes.len(), expected.len());
        for (lane, allocation) in expected {
            let matching: Vec<_> = self.lanes.iter().filter(|returned| returned.id == lane.id).collect();
            assert_eq!(matching.len(), 1);
            assert_eq!(matching[0], lane);
            assert_eq!(matching[0].points.as_ptr(), *allocation);
        }
    }
}

fn assert_applied(fixture: &Fixture, delta: &Delta, completed: &CompletedClipInsertion) {
    assert_eq!(completed.id(), delta.id);
    assert_eq!((completed.track_id, completed.clip_id), (fixture.track, delta.clip));
    assert_eq!(completed.outcome, ClipInsertionOutcome::Applied);
    assert_eq!(std::ptr::from_ref(completed.owner.as_ref()), delta.control);
    assert!(completed.owner.clip.is_none());
    assert!(completed.owner.sender.is_none());
    assert!(completed.owner.receiver.is_abandoned());
    assert_eq!(completed.owner.storage.clips.as_ptr(), delta.eager);
    assert_eq!(completed.owner.storage.removed_audio_clips.as_ptr(), delta.eager_staging);
    let displaced = completed.owner.reply.as_ref().expect("the actual ready reply was consumed");
    assert!(displaced.clips.is_empty());
    assert!(displaced.removed_audio_clips.is_empty());
    assert_eq!(displaced.clips.as_ptr(), fixture.live_allocation);
    assert_eq!(displaced.removed_audio_clips.as_ptr(), fixture.staging_allocation);
    let source = &fixture.engine.tracks()[0].playback_source;
    assert_eq!(source.clips.len(), 2);
    assert!(source.clips.capacity() >= 2);
    assert!(source.removed_audio_clips.capacity() >= 2);
    assert_eq!(Arc::as_ptr(&source.clips[0].audio), fixture.resident.as_ptr());
    let incoming = &source.clips[1];
    assert_eq!(incoming.id, delta.clip);
    assert_eq!(Arc::as_ptr(&incoming.audio), delta.audio.as_ptr());
    assert_eq!(incoming.warp_markers.as_slice().as_ptr(), delta.markers);
    assert_eq!(incoming.warp_markers, delta.expected_markers);
    assert_eq!(fixture.resident.strong_count(), 1);
    assert_eq!(delta.audio.strong_count(), 1);
}

#[test]
fn full_retirement_store_blocks_public_clip_admission_and_preserves_owner_fifo() {
    let mut fixture = setup();
    let mut delta = delta(fixture.track);
    let full_ring = full_ui_ring(&mut fixture.engine);
    let expected = inject_full_retirement_store(&mut fixture.engine);
    let retirement_allocation = fixture.engine.pending_retirements.as_ptr();
    fixture.commands.push(delta.command.take().unwrap()).unwrap();
    fixture.commands.push(EngineCommand::SetBpm(137.0)).unwrap();
    let slots = fixture.commands.slots();
    for block in 0..3 {
        assert_signal(&callback(&mut fixture.engine, true), 0.25);
        assert_eq!(fixture.commands.slots(), slots);
        assert_eq!(fixture.engine.transport().position(), 16 + block * 8);
        assert_eq!(fixture.engine.transport().bpm(), 120.0);
        assert_eq!(fixture.engine.pending_retirements.len(), 32);
        assert_eq!(fixture.engine.pending_retirements.as_ptr(), retirement_allocation);
        assert_eq!(fixture.engine.tracks()[0].playback_source.clips.as_ptr(), fixture.live_allocation);
        assert_eq!(delta.audio.strong_count(), 1);
    }
    assert!(matches!(fixture.events.pop().unwrap(), EngineEvent::PlaybackPosition(u64::MAX)));
    callback(&mut fixture.engine, false);
    assert_eq!(fixture.engine.pending_retirements.len(), 31);
    assert_eq!(fixture.commands.slots(), slots);
    assert_eq!(fixture.engine.transport().position(), 32);
    let mut received = Received::default();
    received.drain(&mut fixture.events);
    assert_eq!(received.fillers, full_ring - 1);
    assert_eq!(received.lanes.len(), 1);
    assert!(received.requests.is_empty());
    callback(&mut fixture.engine, false);
    received.drain(&mut fixture.events);
    received.assert_lanes(&expected);
    assert_eq!(received.requests.len(), 1);
    assert!(received.completed.is_empty());
    assert_eq!(fixture.commands.slots(), slots + 1);
    assert_signal(&callback(&mut fixture.engine, true), 0.25);
    assert_eq!(fixture.engine.transport().bpm(), 120.0);
    assert_eq!(fixture.engine.transport().position(), 40);
    let request = received.requests.pop().unwrap();
    assert_eq!(request.id, delta.id);
    assert_eq!((request.track_id, request.clip_id), (fixture.track, delta.clip));
    request.fulfill();
    callback(&mut fixture.engine, false);
    received.drain(&mut fixture.events);
    assert_eq!(received.completed.len(), 1);
    assert_applied(&fixture, &delta, &received.completed[0]);
    assert_eq!(fixture.engine.transport().bpm(), 137.0);
    assert_eq!(fixture.commands.slots(), slots + 2);
    assert!(fixture.engine.pending_retirements.is_empty());
    assert_eq!(fixture.engine.pending_retirements.capacity(), 32);
    assert_eq!(fixture.engine.pending_retirements.as_ptr(), retirement_allocation);
    assert_signal(&callback(&mut fixture.engine, true), 0.3125);
    assert_eq!(fixture.engine.transport().position(), 48);
    drop(received);
    assert_eq!(delta.audio.strong_count(), 1);
    drop(fixture.engine);
    assert_eq!(delta.audio.strong_count(), 0);
    assert_eq!(fixture.resident.strong_count(), 0);
}
