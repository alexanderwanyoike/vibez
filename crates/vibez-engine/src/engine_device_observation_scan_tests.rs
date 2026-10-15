//! Resident request scans preserve channel order, local recovery and owned-gate suppression.

use super::*;
use graph_test_support::{constant_clip, routing_channel, routing_effect};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use vibez_core::effect::{EffectType, ParamDescriptor};
use vibez_core::id::EffectId;
use vibez_core::midi::InstrumentKind;

const SCAN_ORDER: [usize; 8] = [2, 3, 1, 5, 4, 7, 6, 8];

struct ScanTrace {
    states: [AtomicU32; 8],
    calls: [AtomicU32; 256],
    len: AtomicUsize,
    overflow: AtomicBool,
}

impl ScanTrace {
    fn new() -> Self {
        Self {
            states: std::array::from_fn(|_| AtomicU32::new(0)),
            calls: std::array::from_fn(|_| AtomicU32::new(0)),
            len: AtomicUsize::new(0),
            overflow: AtomicBool::new(false),
        }
    }

    fn clear_calls(&self) {
        self.len.store(0, Ordering::Relaxed);
        self.overflow.store(false, Ordering::Relaxed);
    }

    fn set(&self, owner: usize, state: u32) {
        self.states[owner - 1].store(state, Ordering::Release);
    }

    fn answer(&self, owner: usize, recovery: bool) -> bool {
        let bit = if recovery { 2 } else { 1 };
        let value = self.states[owner - 1].load(Ordering::Acquire) & bit != 0;
        let call = owner as u32 * 100 + if recovery { 20 } else { 10 } + u32::from(value);
        let index = self.len.fetch_add(1, Ordering::Relaxed);
        if let Some(slot) = self.calls.get(index) {
            slot.store(call, Ordering::Relaxed);
        } else {
            self.overflow.store(true, Ordering::Relaxed);
        }
        value
    }

    fn observed(&self) -> Vec<u32> {
        assert!(!self.overflow.load(Ordering::Relaxed));
        self.calls[..self.len.load(Ordering::Relaxed)]
            .iter()
            .map(|call| call.load(Ordering::Relaxed))
            .collect()
    }
    fn assert_request_order(&self, expected: &[usize]) {
        let mut owners = Vec::new();
        for call in self.observed() {
            if call % 100 == 10 || call % 100 == 11 {
                let owner = (call / 100) as usize;
                if !owners.contains(&owner) {
                    owners.push(owner);
                }
            }
        }
        assert_eq!(owners, expected, "resident device order and first blocking owner");
    }

    fn assert_recovery_answer(&self, owner: usize, local: bool) {
        assert!(self.observed().contains(&(owner as u32 * 100 + 20 + u32::from(local))),
            "device {owner} must classify recovery as local = {local}");
    }
}

struct ScanProbe {
    owner: usize,
    trace: Arc<ScanTrace>,
}

impl vibez_dsp::effect::AudioEffect for ScanProbe {
    fn reconfiguration_requested(&self) -> bool {
        self.trace.answer(self.owner, false)
    }
    fn processing_recovery_requested(&self) -> bool {
        self.trace.answer(self.owner, true)
    }
    fn effect_type(&self) -> EffectType {
        EffectType::Gain
    }
    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        &[]
    }
    fn set_param(&mut self, _: usize, _: f32) -> bool {
        false
    }
    fn get_param(&self, _: usize) -> f32 {
        0.0
    }
    fn process(&mut self, _: &mut [f32], _: usize) {}
    fn reset(&mut self) {}
}

impl vibez_instruments::Instrument for ScanProbe {
    fn reconfiguration_requested(&self) -> bool {
        self.trace.answer(self.owner, false)
    }
    fn processing_recovery_requested(&self) -> bool {
        self.trace.answer(self.owner, true)
    }
    fn instrument_kind(&self) -> InstrumentKind {
        InstrumentKind::SubtractiveSynth
    }
    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        &[]
    }
    fn set_param(&mut self, _: usize, _: f32) -> bool {
        false
    }
    fn get_param(&self, _: usize) -> f32 {
        0.0
    }
    fn note_on(&mut self, _: u8, _: u8) {}
    fn note_off(&mut self, _: u8) {}
    fn render(&mut self, buffer: &mut [f32], _: usize) {
        buffer.fill(0.0);
    }
    fn reset(&mut self) {}
}

struct Fixture {
    engine: AudioEngine,
    commands: Producer<EngineCommand>,
    _events: Consumer<EngineEvent>,
    first_track: TrackId,
    trace: Arc<ScanTrace>,
}

fn fixture() -> Fixture {
    let (mut engine, mut commands, events) = AudioEngine::new();
    let trace = Arc::new(ScanTrace::new());
    let tracks = [TrackId::new(), TrackId::new(), TrackId::new()];
    let mut model = Vec::new();
    for (index, (instrument, effects)) in [(1, &[2, 3][..]), (4, &[5][..]), (6, &[7][..])]
        .into_iter()
        .enumerate()
    {
        let mut source = EngineTrack::new(tracks[index]);
        source.instrument = Some(Box::new(ScanProbe {
            owner: instrument,
            trace: Arc::clone(&trace),
        }));
        if index == 0 {
            source.playback_source.clips.push(constant_clip(4096, 0.25));
        }
        let mut channel = routing_channel(tracks[index]);
        channel.is_bus = index == 2;
        for &owner in effects {
            let effect = EffectId::new();
            source.effects.push(EffectSlot {
                id: effect,
                effect: Box::new(ScanProbe {
                    owner,
                    trace: Arc::clone(&trace),
                }),
                bypass: false,
            });
            channel.effects.push(routing_effect(effect));
        }
        commands
            .push(if index == 2 {
                EngineCommand::AddBus(Box::new(source))
            } else {
                EngineCommand::AddTrack(Box::new(source))
            })
            .unwrap();
        model.push(channel);
    }
    let master_effect = EffectId::new();
    commands
        .push(EngineCommand::InsertEffect {
            track_id: TrackId::MASTER,
            effect_id: master_effect,
            effect: Box::new(ScanProbe {
                owner: 8,
                trace: Arc::clone(&trace),
            }),
            position: None,
        })
        .unwrap();
    let mut master = routing_channel(TrackId::MASTER);
    master.effects.push(routing_effect(master_effect));
    model.push(master);
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare(&model, 64).unwrap(),
        ))
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();
    engine.drain_commands();
    assert!(engine.compensation_plan.applied_plan_is_valid());
    assert!(!engine.compensation_plan.awaiting_structural_plan());
    assert!(engine.transport.is_playing());
    trace.clear_calls();
    Fixture {
        engine,
        commands,
        _events: events,
        first_track: tracks[0],
        trace,
    }
}

fn observe(fixture: &Fixture, expected: bool) {
    let mut muted = false;
    assert_eq!(
        crate::retirement::tests::allocations(|| {
            muted = fixture.engine.project_processing_muted();
        }),
        (0, 0)
    );
    assert_eq!(muted, expected);

}

#[test]
fn resident_restart_scan_keeps_effect_instrument_channel_order_and_short_circuits_local_requests() {
    let fixture = fixture();
    observe(&fixture, false);
    fixture.trace.assert_request_order(&SCAN_ORDER);
    fixture.trace.clear_calls();
    for owner in SCAN_ORDER {
        fixture.trace.set(owner, 3);
    }
    observe(&fixture, false);
    fixture.trace.assert_request_order(&SCAN_ORDER);
    for owner in SCAN_ORDER {
        fixture.trace.assert_recovery_answer(owner, true);
    }
    for (target, &selected) in SCAN_ORDER.iter().enumerate() {
        fixture.trace.clear_calls();
        for &owner in &SCAN_ORDER {
            fixture.trace.set(owner, 0);
        }
        for &owner in &SCAN_ORDER[..target] {
            fixture.trace.set(owner, 3);
        }
        fixture.trace.set(selected, 1);
        observe(&fixture, true);
        fixture.trace.assert_request_order(&SCAN_ORDER[..=target]);
        for &owner in &SCAN_ORDER[..target] {
            fixture.trace.assert_recovery_answer(owner, true);
        }
        fixture.trace.assert_recovery_answer(selected, false);
    }
}

#[test]
fn owned_output_gates_suppress_live_plugin_queries_after_real_transitions() {
    for case in ["awaiting", "invalid", "suspended", "presentation"] {
        let mut fixture = fixture();
        match case {
            "awaiting" | "invalid" => {
                fixture
                    .commands
                    .push(EngineCommand::insert_built_in_effect(
                        fixture.first_track,
                        EffectId::new(),
                        EffectType::Gain,
                        DEFAULT_SAMPLE_RATE,
                    ))
                    .unwrap();
                if case == "invalid" {
                    fixture
                        .commands
                        .push(EngineCommand::RejectRoutingUpdate {
                            reason: "Observation baseline rejected plan".into(),
                        })
                        .unwrap();
                }
                fixture.engine.drain_commands();
                assert!(fixture.engine.compensation_plan.awaiting_structural_plan());
                assert_eq!(
                    fixture.engine.compensation_plan.applied_plan_is_valid(),
                    case != "invalid"
                );
            }
            "suspended" => {
                fixture.trace.set(8, 1);
                fixture
                    .engine
                    .process_block(AudioProcessBlock::new(&mut [], 2));
                assert!(fixture.engine.device_handoff.is_suspended());
                assert!(fixture.engine.device_handoff.pending().is_some());
            }
            "presentation" => {
                while fixture
                    .engine
                    .event_tx
                    .push(EngineEvent::PlaybackPosition(0))
                    .is_ok()
                {}
                for _ in 0..=fixture.engine.scheduled_presentation.capacity() {
                    fixture
                        .engine
                        .present_event_after(EngineEvent::PlaybackStarted, 521);
                }
                assert!(fixture.engine.presentation_state.is_faulted());
            }
            _ => unreachable!(),
        }
        for owner in SCAN_ORDER {
            fixture.trace.set(owner, 1);
        }
        fixture.trace.clear_calls();
        observe(&fixture, true);
        assert!(fixture.trace.observed().is_empty(), "owned {case} gate must not query resident devices");
    }
}
