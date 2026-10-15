//! Device request changes reach each real graph segment and the final callback gate.

use super::*;
use graph_test_support::{constant_clip, routing_channel, routing_effect};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use vibez_core::effect::{EffectType, ParamDescriptor};
use vibez_core::id::EffectId;

const QUERY_CLEAR: usize = 1;
const QUERY_REQUESTED: usize = 2;
const RECOVERY_CLEAR: usize = 3;
const PROCESS: usize = 1_000;

struct RequestTrace {
    entries: [AtomicUsize; 256],
    len: AtomicUsize,
    overflow: AtomicBool,
    requested: AtomicBool,
    request_after_process: AtomicBool,
    query_count: AtomicUsize,
    request_on_query: AtomicUsize,
    clear_on_query: AtomicUsize,
}

impl RequestTrace {
    fn new() -> Self {
        Self {
            entries: std::array::from_fn(|_| AtomicUsize::new(0)),
            len: AtomicUsize::new(0),
            overflow: AtomicBool::new(false),
            requested: AtomicBool::new(false),
            request_after_process: AtomicBool::new(false),
            query_count: AtomicUsize::new(0),
            request_on_query: AtomicUsize::new(0),
            clear_on_query: AtomicUsize::new(0),
        }
    }

    fn record(&self, event: usize) {
        let index = self.len.fetch_add(1, Ordering::Relaxed);
        if let Some(entry) = self.entries.get(index) {
            entry.store(event, Ordering::Relaxed);
        } else {
            self.overflow.store(true, Ordering::Relaxed);
        }
    }

    fn arm(&self, after_process: bool, request_on_query: usize, clear_on_query: usize) {
        self.len.store(0, Ordering::Relaxed);
        self.overflow.store(false, Ordering::Relaxed);
        self.requested.store(false, Ordering::Release);
        self.query_count.store(0, Ordering::Relaxed);
        self.request_after_process
            .store(after_process, Ordering::Relaxed);
        self.request_on_query
            .store(request_on_query, Ordering::Relaxed);
        self.clear_on_query.store(clear_on_query, Ordering::Relaxed);
    }

    fn observed(&self) -> Vec<usize> {
        assert!(!self.overflow.load(Ordering::Relaxed));
        self.entries[..self.len.load(Ordering::Relaxed)]
            .iter()
            .map(|entry| entry.load(Ordering::Relaxed))
            .collect()
    }
}

struct RequestProbe(Arc<RequestTrace>);

impl vibez_dsp::effect::AudioEffect for RequestProbe {
    fn reconfiguration_requested(&self) -> bool {
        let query = self.0.query_count.fetch_add(1, Ordering::Relaxed) + 1;
        // Deterministic native interleavings expose stale reuse without relying
        // on a scheduling race or claiming a hosted plug-in clears its own flag.
        if query == self.0.request_on_query.load(Ordering::Relaxed) {
            self.0.requested.store(true, Ordering::Release);
        }
        if query == self.0.clear_on_query.load(Ordering::Relaxed) {
            self.0.requested.store(false, Ordering::Release);
        }
        let requested = self.0.requested.load(Ordering::Acquire);
        self.0.record(if requested {
            QUERY_REQUESTED
        } else {
            QUERY_CLEAR
        });
        requested
    }

    fn processing_recovery_requested(&self) -> bool {
        self.0.record(RECOVERY_CLEAR);
        false
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

    fn process(&mut self, buffer: &mut [f32], channels: usize) {
        self.0.record(PROCESS + buffer.len() / channels);
        if self.0.request_after_process.swap(false, Ordering::Relaxed) {
            self.0.requested.store(true, Ordering::Release);
        }
    }

    fn reset(&mut self) {}
}

struct Fixture {
    engine: AudioEngine,
    commands: Producer<EngineCommand>,
    events: Consumer<EngineEvent>,
    track: TrackId,
    trace: Arc<RequestTrace>,
}

fn fixture(max_frames: usize, idle: bool) -> Fixture {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let effect = EffectId::new();
    let trace = Arc::new(RequestTrace::new());
    let mut source = EngineTrack::new(track);
    source.pan = 0.0;
    if idle {
        source.instrument = Some(Box::new(crate::test_support::DelayProbeInstrument::new(
            0, 0.25,
        )));
    } else {
        source.playback_source.clips.push(constant_clip(4096, 0.25));
    }
    source.effects.push(EffectSlot {
        id: effect,
        effect: Box::new(RequestProbe(Arc::clone(&trace))),
        bypass: false,
    });
    let mut channel = routing_channel(track);
    channel.effects.push(routing_effect(effect));
    commands
        .push(EngineCommand::AddTrack(Box::new(source)))
        .unwrap();
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare(
                &[channel, routing_channel(TrackId::MASTER)],
                max_frames,
            )
            .unwrap(),
        ))
        .unwrap();
    if idle {
        commands
            .push(EngineCommand::ExternalNoteOn {
                track_id: track,
                pitch: 60,
                velocity: 100,
            })
            .unwrap();
    } else {
        commands.push(EngineCommand::Play).unwrap();
    }
    let mut warm = [0.0; 128];
    let mut selected = [0.0; 128];
    engine.process_block(
        AudioProcessBlock::new(&mut warm, 2).with_track_output_capture(track.raw(), &mut selected),
    );
    assert_signal(&warm, 2, |_| true);
    assert_signal(&selected, 2, |_| true);
    assert_eq!(engine.output_gate.last_frame(), [0.25, 0.0]);
    while events.pop().is_ok() {}
    Fixture {
        engine,
        commands,
        events,
        track,
        trace,
    }
}

fn assert_signal(samples: &[f32], channels: usize, audible: impl Fn(usize) -> bool) {
    for (frame, values) in samples.chunks_exact(channels).enumerate() {
        for (channel, sample) in values.iter().enumerate() {
            let expected = if audible(frame) && channel == 0 {
                0.25
            } else {
                0.0
            };
            assert_eq!(*sample, expected, "frame {frame}, channel {channel}");
        }
    }
}

fn render_graph(fixture: &mut Fixture, output: &mut [f32], selected: &mut [f32], channels: usize) {
    let position = fixture.engine.transport.position();
    let frames = output.len() / channels;
    let mut capture = TrackOutputCapture {
        source_track_raw: fixture.track.raw(),
        samples: selected,
    };
    assert_eq!(
        crate::retirement::tests::allocations(|| {
            fixture.engine.render_routing_graph(
                output,
                render_paths::MultitrackRenderBlock {
                    pos: position,
                    repeat_pos: position,
                    frames,
                    channels,
                    loop_region: None,
                    live_input: None,
                },
                Some(&mut capture),
                false,
                true,
            );
        }),
        (0, 0)
    );
}

fn report(
    case: &str,
    channels: usize,
    calls: &[usize],
    output: &[f32],
    selected: &[f32],
    engine: &AudioEngine,
) {
    let output_bits: Vec<_> = output.iter().map(|value| value.to_bits()).collect();
    let selected_bits: Vec<_> = selected.iter().map(|value| value.to_bits()).collect();
    eprintln!("device-observation-a: case={case}; channels={channels}; calls={calls:?}; arrange={}; performance={}; output_position={}; playing={}; loss={}; resume={}; output_bits={output_bits:?}; selected_bits={selected_bits:?}",
        engine.transport.position(), engine.performance_position, engine.output_position,
        engine.transport.is_playing(), engine.output_gate.remaining_loss_frames(),
        engine.output_gate.remaining_resume_frames());
}

#[test]
fn oversized_graph_segments_observe_new_and_cleared_requests_without_reusing_parent_permission() {
    for channels in [1, 2, 8] {
        for clear_later in [false, true] {
            let mut fixture = fixture(8, false);
            // Wide children reenter stereo: read4 blocks the second child,
            // then read5 belongs to the third child's outer gate.
            let clear_on_query = if clear_later {
                if channels > 2 {
                    5
                } else {
                    4
                }
            } else {
                0
            };
            fixture.trace.arm(true, 0, clear_on_query);
            let mut output = vec![-123.0; 24 * channels];
            let mut selected = vec![-123.0; 24 * channels];
            render_graph(&mut fixture, &mut output, &mut selected, channels);
            let audible = |frame| frame < 8 || (clear_later && frame >= 16);
            assert_signal(&output, channels, audible);
            assert_signal(&selected, channels, audible);
            let mut expected = vec![QUERY_CLEAR, QUERY_CLEAR];
            if channels > 2 {
                expected.push(QUERY_CLEAR);
            }
            expected.extend([PROCESS + 8, QUERY_REQUESTED, RECOVERY_CLEAR]);
            if clear_later {
                expected.push(QUERY_CLEAR);
                if channels > 2 {
                    expected.push(QUERY_CLEAR);
                }
                expected.push(PROCESS + 8);
            } else {
                expected.extend([QUERY_REQUESTED, RECOVERY_CLEAR]);
            }
            let calls = fixture.trace.observed();
            assert_eq!(
                calls, expected,
                "channels={channels}, clear_later={clear_later}"
            );
            assert_eq!(fixture.engine.transport.position(), 64);
            assert_eq!(fixture.engine.output_position, 64);
            report(
                if clear_later {
                    "recursive-clear"
                } else {
                    "recursive-request"
                },
                channels,
                &calls,
                &output,
                &selected,
                &fixture.engine,
            );
        }
    }
}

#[test]
fn wider_layout_reentry_observes_a_request_after_its_outer_gate_and_zeros_both_outputs() {
    for channels in [4, 8] {
        let mut fixture = fixture(8, false);
        fixture.trace.arm(false, 2, 0);
        let mut output = vec![-123.0; 8 * channels];
        let mut selected = vec![-123.0; 8 * channels];
        render_graph(&mut fixture, &mut output, &mut selected, channels);
        assert!(output.iter().all(|sample| *sample == 0.0));
        assert!(selected.iter().all(|sample| *sample == 0.0));
        let calls = fixture.trace.observed();
        assert_eq!(calls, [QUERY_CLEAR, QUERY_REQUESTED, RECOVERY_CLEAR]);
        assert_eq!(fixture.engine.transport.position(), 64);
        report(
            "layout-reentry",
            channels,
            &calls,
            &output,
            &selected,
            &fixture.engine,
        );
    }
}

#[test]
fn empty_graph_entries_keep_the_original_request_observation_without_processing_audio() {
    for channels in [1, 2, 8] {
        let mut fixture = fixture(8, false);
        fixture.trace.arm(false, 0, 0);
        render_graph(&mut fixture, &mut [], &mut [], channels);
        let calls = fixture.trace.observed();
        assert_eq!(
            calls,
            if channels > 2 {
                vec![QUERY_CLEAR, QUERY_CLEAR]
            } else {
                vec![QUERY_CLEAR]
            }
        );
        assert_eq!(fixture.engine.transport.position(), 64);
        assert_eq!(fixture.engine.output_position, 64);
        report("empty-entry", channels, &calls, &[], &[], &fixture.engine);
    }
}

#[derive(Clone, Copy, Debug)]
enum CallbackSource {
    Arrange,
    IdleInstrument,
    ArrangeLoop,
}

#[test]
fn callback_final_gate_observes_the_last_process_request_and_preserves_loss_fade_and_capture_silence(
) {
    for mode in [
        CallbackSource::Arrange,
        CallbackSource::IdleInstrument,
        CallbackSource::ArrangeLoop,
    ] {
        let idle = matches!(mode, CallbackSource::IdleInstrument);
        let mut fixture = fixture(128, idle);
        if matches!(mode, CallbackSource::ArrangeLoop) {
            fixture
                .commands
                .push(EngineCommand::SetArrangementLoopRegion { start: 64, end: 80 })
                .unwrap();
            fixture
                .commands
                .push(EngineCommand::SetArrangementLoop(true))
                .unwrap();
        }
        fixture.trace.arm(true, 0, 0);
        let mut output = [0.0; 192];
        let mut selected = [-123.0; 192];
        assert_eq!(
            crate::retirement::tests::allocations(|| {
                fixture.engine.process_block(
                    AudioProcessBlock::new(&mut output, 2)
                        .with_track_output_capture(fixture.track.raw(), &mut selected),
                );
            }),
            (0, 0)
        );
        for (frame, values) in output.as_chunks::<2>().0.iter().enumerate() {
            let expected = if frame < 64 {
                0.25 * (64 - frame) as f32 / 64.0
            } else {
                0.0
            };
            assert_eq!(values[0], expected, "{mode:?}, loss frame {frame}");
            assert_eq!(values[1], 0.0);
        }
        assert!(selected.iter().all(|sample| *sample == 0.0));
        assert_eq!(fixture.engine.output_gate.remaining_loss_frames(), 0);
        assert_eq!(fixture.engine.output_gate.remaining_resume_frames(), 64);
        assert!(fixture.engine.compensation_plan.applied_plan_is_valid());
        assert!(fixture.engine.device_handoff.pending().is_none());
        assert_eq!(fixture.engine.transport.is_playing(), !idle);
        let expected_position = match mode {
            CallbackSource::Arrange => 160,
            CallbackSource::IdleInstrument => 0,
            CallbackSource::ArrangeLoop => 64,
        };
        assert_eq!(fixture.engine.transport.position(), expected_position);
        assert_eq!(
            fixture.engine.performance_position,
            if idle { 160 } else { expected_position }
        );
        assert_eq!(fixture.engine.output_position, 160);
        let calls = fixture.trace.observed();
        let expected = if matches!(mode, CallbackSource::ArrangeLoop) {
            vec![
                QUERY_CLEAR,
                QUERY_CLEAR,
                PROCESS + 16,
                QUERY_REQUESTED,
                RECOVERY_CLEAR,
                QUERY_REQUESTED,
                RECOVERY_CLEAR,
            ]
        } else {
            vec![
                QUERY_CLEAR,
                QUERY_CLEAR,
                PROCESS + 96,
                QUERY_REQUESTED,
                RECOVERY_CLEAR,
            ]
        };
        assert_eq!(calls, expected, "{mode:?}");
        while let Ok(event) = fixture.events.pop() {
            assert!(!matches!(
                event,
                EngineEvent::DeviceReconfiguration(_)
                    | EngineEvent::CompensationFailed { .. }
                    | EngineEvent::CompensationInvalid { .. }
                    | EngineEvent::PlaybackStopped
            ));
        }
        report(
            match mode {
                CallbackSource::Arrange => "final-arrange",
                CallbackSource::IdleInstrument => "final-idle",
                CallbackSource::ArrangeLoop => "final-loop",
            },
            2,
            &calls,
            &output,
            &selected,
            &fixture.engine,
        );
    }
}
