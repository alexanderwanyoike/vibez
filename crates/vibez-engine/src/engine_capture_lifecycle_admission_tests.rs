//! Capture command acknowledgements preserve admission priority independently of local episode closure.

use super::*;
use graph_test_support::{constant_clip, routing_channel, routing_effect};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use vibez_core::effect::{EffectType, ParamDescriptor};
use vibez_core::id::EffectId;
use vibez_core::midi::InstrumentKind;

const EFFECT_PROBE_OWNER: u32 = 2;

pub(super) struct Calls {
    values: [AtomicU32; 64],
    len: AtomicUsize,
    overflow: AtomicBool,
}

impl Calls {
    fn new() -> Self {
        Self {
            values: std::array::from_fn(|_| AtomicU32::new(0)),
            len: AtomicUsize::new(0),
            overflow: AtomicBool::new(false),
        }
    }
    fn record(&self, owner: u32, local: bool) {
        let index = self.len.fetch_add(1, Ordering::Relaxed);
        if let Some(value) = self.values.get(index) {
            value.store(owner * 100 + 10 + u32::from(local), Ordering::Relaxed);
        } else {
            self.overflow.store(true, Ordering::Relaxed);
        }
    }
    pub(super) fn clear(&self) {
        self.len.store(0, Ordering::Relaxed);
        self.overflow.store(false, Ordering::Relaxed);
    }
    pub(super) fn observed(&self) -> Vec<u32> {
        assert!(!self.overflow.load(Ordering::Relaxed));
        self.values[..self.len.load(Ordering::Relaxed)]
            .iter()
            .map(|value| value.load(Ordering::Relaxed))
            .collect()
    }
    fn assert_local_answer(&self, owner: u32, local: bool) {
        let answer = owner * 100 + 10 + u32::from(local);
        assert!(
            self.observed().contains(&answer),
            "device {owner} must be observed with local failure = {local}"
        );
    }
    pub(super) fn assert_effect_failure_observed(&self, local: bool) {
        self.assert_local_answer(EFFECT_PROBE_OWNER, local);
    }
    pub(super) fn assert_effect_recovered_after_failure(&self) {
        let calls = self.observed();
        let failure = calls
            .iter()
            .position(|answer| *answer == EFFECT_PROBE_OWNER * 100 + 11)
            .expect("the first Start must observe the local failure");
        assert!(
            calls[failure + 1..].contains(&(EFFECT_PROBE_OWNER * 100 + 10)),
            "the later Start must observe the recovery edit"
        );
    }
    pub(super) fn assert_effect_recovered_before_refailure(&self) {
        let calls = self.observed();
        let recovered = EFFECT_PROBE_OWNER * 100 + 10;
        let recovery = calls
            .iter()
            .position(|answer| *answer == recovered)
            .expect("Capture admission must observe the prior recovery edit");
        assert!(
            calls[recovery + 1..].contains(&(recovered + 1)),
            "validation must observe the later same-drain refailure"
        );
    }
}

struct Control {
    local: AtomicBool,
    offer: AtomicBool,
    valid: AtomicBool,
    drops: AtomicUsize,
}
impl Control {
    fn new() -> Self {
        Self {
            local: AtomicBool::new(false),
            offer: AtomicBool::new(false),
            valid: AtomicBool::new(true),
            drops: AtomicUsize::new(0),
        }
    }
}

struct Probe {
    owner: u32,
    control: Arc<Control>,
    calls: Arc<Calls>,
}
impl Drop for Probe {
    fn drop(&mut self) {
        self.control.drops.fetch_add(1, Ordering::Relaxed);
    }
}

macro_rules! observations {
    () => {
        fn processing_failure_is_local(&self) -> bool {
            let local = self.control.local.load(Ordering::Acquire);
            self.calls.record(self.owner, local);
            local
        }
        fn processing_configuration_valid(&self) -> bool {
            !self.control.local.load(Ordering::Acquire)
                && self.control.valid.load(Ordering::Acquire)
        }
        fn reconfiguration_requested(&self) -> bool {
            self.control.offer.load(Ordering::Acquire)
        }
        fn processing_recovery_requested(&self) -> bool {
            self.control.offer.load(Ordering::Acquire) && self.control.local.load(Ordering::Acquire)
        }
        fn param_descriptors(&self) -> &'static [ParamDescriptor] {
            &[]
        }
        fn set_param(&mut self, index: usize, value: f32) -> bool {
            match index {
                0 => self.control.local.store(value != 0.0, Ordering::Release),
                1 => self.control.offer.store(value != 0.0, Ordering::Release),
                2 => self.control.valid.store(value != 0.0, Ordering::Release),
                _ => return false,
            }
            true
        }
        fn get_param(&self, _: usize) -> f32 {
            0.0
        }
        fn reset(&mut self) {}
    };
}

impl vibez_dsp::effect::AudioEffect for Probe {
    observations!();
    fn effect_type(&self) -> EffectType {
        EffectType::Gain
    }
    fn process(&mut self, _: &mut [f32], _: usize) {}
}
impl vibez_instruments::Instrument for Probe {
    observations!();
    fn instrument_kind(&self) -> InstrumentKind {
        InstrumentKind::SubtractiveSynth
    }
    fn note_on(&mut self, _: u8, _: u8) {}
    fn note_off(&mut self, _: u8) {}
    fn render(&mut self, buffer: &mut [f32], _: usize) {
        buffer.fill(0.0);
    }
}

pub(super) struct Fixture {
    pub(super) engine: AudioEngine,
    pub(super) commands: Producer<EngineCommand>,
    pub(super) events: Consumer<EngineEvent>,
    pub(super) tracks: [TrackId; 2],
    pub(super) effect: EffectId,
    controls: [Arc<Control>; 2],
    pub(super) calls: Arc<Calls>,
    audio: std::sync::Weak<DecodedAudio>,
    pub(super) owner: Option<reconfiguration::DeviceReconfiguration>,
    effect_identity: usize,
}

pub(super) fn fixture() -> Fixture {
    let tracks = [TrackId::new(), TrackId::new()];
    let effect = EffectId::new();
    let controls = [Arc::new(Control::new()), Arc::new(Control::new())];
    let calls = Arc::new(Calls::new());
    let mut affected = EngineTrack::new(tracks[0]);
    affected.pan = 0.0;
    affected.instrument = Some(Box::new(Probe {
        owner: 1,
        control: Arc::clone(&controls[0]),
        calls: Arc::clone(&calls),
    }));
    let device: Box<dyn vibez_dsp::effect::AudioEffect> = Box::new(Probe {
        owner: EFFECT_PROBE_OWNER,
        control: Arc::clone(&controls[1]),
        calls: Arc::clone(&calls),
    });
    let effect_identity = reconfiguration::effect_owner_identity(&*device);
    affected.effects.push(EffectSlot {
        id: effect,
        effect: device,
        bypass: false,
    });
    let mut healthy = EngineTrack::new(tracks[1]);
    healthy.pan = 0.0;
    let clip = constant_clip(4096, 0.25);
    let audio = Arc::downgrade(&clip.audio);
    healthy.playback_source.clips.push(clip);
    let mut channel = routing_channel(tracks[0]);
    channel.effects.push(routing_effect(effect));
    let model = [
        channel,
        routing_channel(tracks[1]),
        routing_channel(TrackId::MASTER),
    ];
    let (mut engine, mut commands, events) = AudioEngine::new();
    commands
        .push(EngineCommand::AddTrack(Box::new(affected)))
        .unwrap();
    commands
        .push(EngineCommand::AddTrack(Box::new(healthy)))
        .unwrap();
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare(&model, 64).unwrap(),
        ))
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();
    engine.drain_commands();
    let mut fixture = Fixture {
        engine,
        commands,
        events,
        tracks,
        effect,
        controls,
        calls,
        audio,
        owner: None,
        effect_identity,
    };
    discard_setup(&mut fixture);
    fixture.calls.clear();
    fixture
}

fn set(fixture: &Fixture, instrument: bool, index: usize) -> EngineCommand {
    if instrument {
        EngineCommand::SetInstrumentParam {
            track_id: fixture.tracks[0],
            param_index: index,
            value: 1.0,
        }
    } else {
        EngineCommand::SetEffectParam {
            track_id: fixture.tracks[0],
            effect_id: fixture.effect,
            param_index: index,
            value: 1.0,
        }
    }
}

pub(super) fn discard_setup(fixture: &mut Fixture) {
    while let Ok(event) = fixture.events.pop() {
        if let EngineEvent::DeviceReconfiguration(owner) = event {
            assert!(fixture.owner.is_none());
            assert_eq!(owner.effect_id(), Some(fixture.effect));
            assert_eq!(owner.owner_identity(), fixture.effect_identity);
            fixture.owner = Some(owner);
        }
    }
}

pub(super) fn state(engine: &AudioEngine) -> [u64; 7] {
    [
        u64::from(engine.capture_lifecycle.is_active()),
        u64::from(engine.capture_lifecycle.local_failure_closure_requested()),
        u64::from(engine.compensation_plan.applied_plan_is_valid()),
        u64::from(engine.transport.is_playing()),
        engine.transport.position(),
        engine.performance_position,
        engine.output_position,
    ]
}

#[derive(Default)]
struct Events {
    order: Vec<&'static str>,
    reasons: Vec<&'static str>,
    times: Vec<u64>,
    sources: Vec<usize>,
    offset_owners: usize,
}

fn collect(fixture: &mut Fixture) -> Events {
    let mut result = Events::default();
    while let Ok(event) = fixture.events.pop() {
        match event {
            EngineEvent::PerformanceCaptureStarted {
                offsets,
                effective_at_samples,
                section_id,
                section_position_samples,
            } => {
                assert!(Arc::ptr_eq(
                    &offsets,
                    &fixture.engine.routing.capture_offsets
                ));
                assert_eq!((section_id, section_position_samples), (None, None));
                result.offset_owners += 1;
                result.order.push("start");
                result.times.push(effective_at_samples);
            }
            EngineEvent::ClipCaptureSource {
                track_id,
                position,
                effective_at_samples,
            } => {
                assert_eq!(position, 0);
                result.sources.push(
                    fixture
                        .tracks
                        .iter()
                        .position(|track| *track == track_id)
                        .unwrap(),
                );
                result.order.push("source");
                result.times.push(effective_at_samples);
            }
            EngineEvent::PerformanceCaptureStopped {
                effective_at_samples,
            } => {
                result.order.push("stop");
                result.times.push(effective_at_samples);
            }
            EngineEvent::CompensationInvalid {
                track_id,
                effect_id,
                reason,
            } => {
                assert_eq!((track_id, effect_id), (TrackId::MASTER, None));
                result.order.push("refused");
                result.reasons.push(reason);
            }
            EngineEvent::TrackMeter { .. }
            | EngineEvent::Metering { .. }
            | EngineEvent::PlaybackPosition(_) => {}
            event => panic!("unexpected Capture lifecycle event: {event:?}"),
        }
    }
    assert!(result.times.iter().all(|time| *time == 0));
    result
}

enum AdmissionObservation {
    Healthy,
    EffectFailure,
    InstrumentFailure,
    OwnedGate,
}

struct Expected {
    observation: AdmissionObservation,
    order: Vec<&'static str>,
    reason: Option<&'static str>,
    active: bool,
    latched: bool,
    valid: bool,
    follow_close: bool,
}

fn exercise(fixture: &mut Fixture, _case: &str, commands: &[bool], expected: Expected) {
    let Expected {
        observation,
        order: expected_order,
        reason,
        active,
        latched,
        valid,
        follow_close,
    } = expected;
    let before = state(&fixture.engine);
    assert_eq!(
        before,
        [
            0,
            u64::from(latched),
            u64::from(valid),
            u64::from(valid),
            0,
            0,
            0
        ]
    );
    let offsets = Arc::strong_count(&fixture.engine.routing.capture_offsets);
    for start in commands {
        fixture
            .commands
            .push(if *start {
                EngineCommand::StartPerformanceCapture
            } else {
                EngineCommand::StopPerformanceCapture
            })
            .unwrap();
    }
    fixture.calls.clear();
    assert_eq!(
        crate::retirement::tests::allocations(|| fixture.engine.drain_commands()),
        (0, 0)
    );
    assert!(fixture.engine.cmd_rx.peek().is_err());
    match observation {
        AdmissionObservation::Healthy => {
            fixture.calls.assert_local_answer(1, false);
            fixture.calls.assert_effect_failure_observed(false);
        }
        AdmissionObservation::EffectFailure => fixture.calls.assert_effect_failure_observed(true),
        AdmissionObservation::InstrumentFailure => {
            fixture.calls.assert_local_answer(1, true);
            assert!(
                fixture
                    .calls
                    .observed()
                    .iter()
                    .all(|answer| answer / 100 == 1),
                "the first local instrument failure must short-circuit the effect scan"
            );
        }
        AdmissionObservation::OwnedGate => assert!(fixture.calls.observed().is_empty()),
    }
    let after = state(&fixture.engine);
    assert_eq!(
        after,
        [
            u64::from(active),
            u64::from(latched),
            u64::from(valid),
            u64::from(valid),
            0,
            0,
            0
        ]
    );
    let starts = if reason.is_none() {
        commands.iter().filter(|start| **start).count()
    } else {
        0
    };
    assert_eq!(
        Arc::strong_count(&fixture.engine.routing.capture_offsets),
        offsets + starts
    );
    let observed = collect(fixture);
    assert_eq!(observed.order, expected_order);
    assert_eq!(
        observed.reasons,
        if let Some(reason) = reason {
            vec![reason; 2]
        } else {
            Vec::new()
        }
    );
    assert_eq!(observed.offset_owners, starts);
    assert_eq!(observed.sources, [0, 1].repeat(starts));
    assert_eq!(
        Arc::strong_count(&fixture.engine.routing.capture_offsets),
        offsets
    );
    let mut output = [0.0; 16];
    assert_eq!(
        crate::retirement::tests::allocations(|| fixture
            .engine
            .process_block(AudioProcessBlock::new(&mut output, 2))),
        (0, 0)
    );
    for frame in output.as_chunks::<2>().0 {
        assert_eq!(*frame, [if valid { 0.25 } else { 0.0 }, 0.0]);
    }
    let followed = collect(fixture);
    assert_eq!(
        followed.order,
        if follow_close {
            vec!["stop"]
        } else {
            Vec::new()
        }
    );
    assert!(followed.reasons.is_empty());
    assert!(followed.sources.is_empty());
    assert_eq!(followed.offset_owners, 0);
    let final_state = state(&fixture.engine);
    assert_eq!(
        final_state,
        [
            u64::from(active),
            u64::from(latched || follow_close),
            u64::from(valid),
            u64::from(valid),
            if valid { 8 } else { 0 },
            8,
            8
        ]
    );
    assert_eq!(
        fixture
            .controls
            .iter()
            .map(|control| control.drops.load(Ordering::Relaxed))
            .collect::<Vec<_>>(),
        [0, 0]
    );
    assert_eq!(fixture.audio.strong_count(), 1);
}

pub(super) fn teardown(fixture: Fixture) {
    let controls = fixture.controls.clone();
    let audio = fixture.audio.clone();
    drop(fixture);
    assert!(controls
        .iter()
        .all(|control| control.drops.load(Ordering::Relaxed) == 1));
    assert_eq!(audio.strong_count(), 0);
}

#[test]
fn healthy_repeated_starts_and_stops_keep_each_public_acknowledgement() {
    for (case, commands, order, active) in [
        (
            "start-start-stop-stop",
            vec![true, true, false, false],
            vec![
                "start", "source", "source", "start", "source", "source", "stop", "stop",
            ],
            false,
        ),
        (
            "start-stop-start",
            vec![true, false, true],
            vec![
                "start", "source", "source", "stop", "start", "source", "source",
            ],
            true,
        ),
    ] {
        let mut fixture = fixture();
        exercise(
            &mut fixture,
            case,
            &commands,
            Expected {
                observation: AdmissionObservation::Healthy,
                order,
                reason: None,
                active,
                latched: false,
                valid: true,
                follow_close: false,
            },
        );
        teardown(fixture);
    }
}

#[test]
fn refused_starts_and_explicit_stop_preserve_local_priority_and_episode_independence() {
    for case in [
        "effect-local",
        "instrument-local",
        "invalid",
        "local-and-invalid",
        "local-closed",
        "local-token",
    ] {
        let mut fixture = fixture();
        let invalid = matches!(case, "invalid" | "local-and-invalid");
        let instrument = matches!(case, "instrument-local" | "local-and-invalid");
        if case != "invalid" {
            fixture.commands.push(set(&fixture, instrument, 0)).unwrap();
        }
        if case == "local-token" {
            fixture.commands.push(set(&fixture, false, 1)).unwrap();
        }
        if invalid {
            fixture
                .commands
                .push(EngineCommand::insert_built_in_effect(
                    fixture.tracks[0],
                    EffectId::new(),
                    EffectType::Gain,
                    DEFAULT_SAMPLE_RATE,
                ))
                .unwrap();
            fixture
                .commands
                .push(EngineCommand::RejectRoutingUpdate {
                    reason: "Capture lifecycle original setup".into(),
                })
                .unwrap();
        }
        fixture.engine.drain_commands();
        discard_setup(&mut fixture);
        let latched = matches!(case, "local-closed" | "local-token");
        if latched {
            fixture
                .engine
                .process_block(AudioProcessBlock::new(&mut [], 2));
            discard_setup(&mut fixture);
        }
        assert_eq!(
            fixture
                .engine
                .capture_lifecycle
                .local_failure_closure_requested(),
            latched
        );
        assert_eq!(fixture.owner.is_some(), case == "local-token");
        let observation = if case == "local-token" {
            AdmissionObservation::OwnedGate
        } else if instrument {
            AdmissionObservation::InstrumentFailure
        } else if case == "invalid" {
            AdmissionObservation::Healthy
        } else {
            AdmissionObservation::EffectFailure
        };
        let reason = if case == "invalid" {
            "Capture requires a valid applied audio configuration"
        } else {
            "Capture requires recovery or reload of the failed device"
        };
        exercise(
            &mut fixture,
            case,
            &[true, true, false],
            Expected {
                observation,
                order: vec!["stop", "refused", "stop", "refused", "stop"],
                reason: Some(reason),
                active: false,
                latched,
                valid: !invalid,
                follow_close: matches!(case, "effect-local" | "instrument-local"),
            },
        );
        teardown(fixture);
    }
}
