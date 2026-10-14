//! Native DSP is a dispatch oracle here; independent signal mathematics lives in the DSP suites.

use super::*;
use std::sync::Arc;
use vibez_core::{
    audio_buffer::DecodedAudio,
    automation::{AutomationLane, AutomationPoint, AutomationTarget},
    id::ClipId,
};
use vibez_dsp::{effect::AudioEffect, factory::create_effect_for_instance};

const OLD: [f32; 9] = [0.5, 0.0, 0.25, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5];
const NEW: [f32; 9] = [0.25, 0.125, 0.75, 0.25, 0.5, 1.0, 0.5, 1.0, 0.125];
const FINAL: [f32; 9] = [0.75, 0.25, 0.0, 0.5, 1.0, 0.0, 0.25, 0.0, 0.75];
const RANGES: [(f32, f32); 9] = [
    (2.0, 16.0),
    (1.0, 32.0),
    (0.0, 1.0),
    (0.0, 24.0),
    (0.0, 2.0),
    (0.0, 1.0),
    (0.0, 1.0),
    (0.0, 1.0),
    (20.0, 20_000.0),
];

fn native(normalized: [f32; 9]) -> [f32; 9] {
    std::array::from_fn(|index| {
        let (min, max) = RANGES[index];
        min + normalized[index] * (max - min)
    })
}

fn lane(effect: EffectId, index: usize, old: f32, new: f32, final_value: f32) -> AutomationLane {
    let mut lane = AutomationLane::new(AutomationTarget::EffectParam {
        effect_id: effect,
        param_index: index,
    });
    for (sample, value) in [
        (0, old),
        (959, old),
        (960, new),
        (1215, new),
        (1216, final_value),
    ] {
        lane.insert_point(AutomationPoint {
            beat: sample as f64 / 24_000.0,
            value,
            curve: 0.0,
        });
    }
    lane
}

fn prepare(graph: bool, analytical: bool) -> (Fixture, Box<dyn AudioEffect>, Vec<AutomationLane>) {
    let mut fixture = Fixture::new(graph, Owner::Track);
    let normalized = if analytical { [0.0; 9] } else { OLD };
    let params = native(normalized);
    for (param_index, value) in params.into_iter().enumerate() {
        fixture
            .commands
            .push(EngineCommand::SetEffectParam {
                track_id: fixture.selected,
                effect_id: fixture.effect,
                param_index,
                value,
            })
            .unwrap();
    }
    fixture
        .commands
        .push(EngineCommand::AddClip {
            track_id: fixture.source,
            clip_id: ClipId::new(),
            audio: Arc::new(DecodedAudio {
                channels: vec![vec![0.0; 4096]; 2],
                sample_rate: 48_000,
            }),
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
        })
        .unwrap();
    let mut prepared = Vec::new();
    for index in 0..9 {
        if analytical && index != 2 {
            continue;
        }
        let complete = if analytical {
            lane(fixture.effect, index, 0.0, 1.0, 1.0)
        } else {
            lane(fixture.effect, index, OLD[index], NEW[index], FINAL[index])
        };
        let initial = if analytical {
            complete.clone()
        } else {
            let mut empty = complete.clone();
            empty.points.clear();
            empty
        };
        fixture
            .commands
            .push(EngineCommand::SetAutomationLane {
                track_id: fixture.source,
                lane: initial,
            })
            .unwrap();
        prepared.push(complete);
    }
    if graph {
        let channels = [
            RoutingChannel {
                id: fixture.source,
                is_bus: false,
                sends: vec![],
                effects: vec![RoutingEffect {
                    id: fixture.effect,
                    inputs: vec![],
                    assignments: vec![],
                    inactive_inputs: vec![],
                }],
            },
            RoutingChannel {
                id: TrackId::MASTER,
                is_bus: true,
                sends: vec![],
                effects: vec![],
            },
        ];
        let reports = [(
            RoutingNode {
                channel: fixture.source,
                stage: NodeStage::Effect(fixture.effect),
            },
            0,
        )];
        let mut plan =
            crate::routing::PreparedRouting::prepare_compensated(&channels, 128, &reports, &[], 2)
                .unwrap();
        let targets: Vec<_> = prepared
            .iter()
            .map(|lane| (fixture.source, lane.target))
            .collect();
        plan.configure_automation(&targets).unwrap();
        fixture
            .commands
            .push(EngineCommand::SetRouting(plan))
            .unwrap();
    }
    fixture.commands.push(EngineCommand::SetBpm(120.0)).unwrap();
    fixture.select(true, 41);
    fixture.commands.push(EngineCommand::Play).unwrap();
    let reference =
        create_effect_for_instance(EffectType::Bitcrush, 48_000.0, &params, fixture.effect);
    (fixture, reference, prepared)
}

#[derive(Default)]
struct WindowLevels {
    frames: usize,
    peak: [[f32; 2]; 2],
    squares: [[f64; 2]; 2],
}

impl WindowLevels {
    fn observe(&mut self, input: &[f32], output: &[f32]) {
        for (dry, wet) in input.as_chunks::<2>().0.iter().zip(output.as_chunks::<2>().0.iter()) {
            for (side, pair) in [dry, wet].into_iter().enumerate() {
                for (channel, &value) in pair.iter().enumerate() {
                    self.peak[side][channel] = self.peak[side][channel].max(value.abs());
                    self.squares[side][channel] += f64::from(value).powi(2);
                }
            }
            self.frames += 1;
        }
    }

    fn check(&self, input: MeterValues, output: MeterValues) {
        for (side, meter) in [input, output].into_iter().enumerate() {
            for (actual, expected) in [
                (meter.peak_l, self.peak[side][0]),
                (meter.peak_r, self.peak[side][1]),
                (
                    meter.rms_l,
                    (self.squares[side][0] / self.frames as f64).sqrt() as f32,
                ),
                (
                    meter.rms_r,
                    (self.squares[side][1] / self.frames as f64).sqrt() as f32,
                ),
            ] {
                assert!(
                    (actual - expected).abs() < 2e-6,
                    "actual level={actual}, expected={expected}"
                );
            }
        }
    }
}

fn check_callback(
    fixture: &mut Fixture,
    reference: &mut dyn AudioEffect,
    position: usize,
    automated: bool,
    analytical: bool,
    levels: &mut WindowLevels,
) -> Observed {
    let mut input = [0.0; 256];
    for (frame, pair) in input.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        let value = if analytical {
            [0.3, -0.19]
        } else {
            let left = (((position + frame) % 137) as f32 - 68.0) / 97.0;
            [left, 0.173 - left * 0.37]
        };
        pair.copy_from_slice(&value);
    }
    let mut expected = input;
    let segment = if fixture.graph { 64 } else { 128 };
    for offset in (0..128).step_by(segment) {
        if automated {
            let sample = position + offset;
            if analytical {
                assert!(reference.set_automated_param(2, if sample < 960 { 0.0 } else { 1.0 }));
            } else {
                let values = native(if sample < 960 {
                    OLD
                } else if sample < 1216 {
                    NEW
                } else {
                    FINAL
                });
                for (index, value) in values.into_iter().enumerate() {
                    assert!(reference.set_automated_param(index, value));
                }
            }
        }
        reference.process(&mut expected[offset * 2..(offset + segment) * 2], 2);
    }
    levels.observe(&input, &expected);
    let mut output = [0.0; 256];
    let mut captured = [0.0; 256];
    let traffic = crate::retirement::tests::allocations(|| {
        fixture.engine.process_block(
            AudioProcessBlock::new(&mut output, 2)
                .with_live_input(fixture.source.raw(), &input)
                .with_track_output_capture(fixture.source.raw(), &mut captured),
        );
    });
    // Setup may grow generic source storage; every subsequent actual dispatch is measured.
    if position == 0 {
        println!(
            "graph={}, setup-only callback heap={traffic:?}",
            fixture.graph
        );
    } else {
        assert_eq!(
            traffic,
            (0, 0),
            "source frame={position}, graph={}",
            fixture.graph
        );
    }
    for sample in 0..256 {
        let wanted = expected[sample] * std::f32::consts::FRAC_1_SQRT_2;
        assert!(
            (captured[sample] - wanted).abs() < 2e-6,
            "dispatch frame={}, channel={}, graph={}, actual={}, expected={wanted}",
            position + sample / 2,
            sample % 2,
            fixture.graph,
            captured[sample]
        );
    }
    if analytical {
        let boundary = if fixture.graph { 960 } else { 1024 };
        for (point, pair) in captured.as_chunks::<2>().0.iter().enumerate() {
            let wet = if position + point < boundary {
                [0.3, -0.19]
            } else {
                [0.5, 0.0]
            };
            for channel in 0..2 {
                let wanted = wet[channel] * std::f32::consts::FRAC_1_SQRT_2;
                assert!(
                    (pair[channel] - wanted).abs() < 1e-6,
                    "analytical source boundary frame{}",
                    position + point
                );
            }
        }
    }
    let observed = fixture.observe();
    assert_eq!(observed.scopes.len(), observed.levels.len());
    for &(track, effect, generation, meter) in &observed.scopes {
        assert_eq!(
            (track, effect, generation),
            (fixture.selected, fixture.effect, 41)
        );
        assert_eq!(
            meter,
            reference.take_sample_hold_meter().unwrap(),
            "actual selected scope sourceframe{position}"
        );
    }
    for &(track, effect, generation, input, output) in &observed.levels {
        assert_eq!(
            (track, effect, generation),
            (fixture.selected, fixture.effect, 41)
        );
        levels.check(input, output);
        *levels = WindowLevels::default();
    }
    observed
}

#[test]
fn nine_manual_fields_and_real_normalized_lanes_match_actual_dispatch_during_selected_transitions()
{
    for graph in [false, true] {
        let (mut fixture, mut reference, prepared) = prepare(graph, false);
        let mut levels = WindowLevels::default();
        let mut prepared = Some(prepared);
        let mut windows = 0;
        for block in 0..16 {
            let position = block * 128;
            if block == 1 || block == 3 {
                for (index, value) in native(if block == 1 { NEW } else { OLD })
                    .into_iter()
                    .enumerate()
                {
                    fixture
                        .commands
                        .push(EngineCommand::SetEffectParam {
                            track_id: fixture.selected,
                            effect_id: fixture.effect,
                            param_index: index,
                            value,
                        })
                        .unwrap();
                    assert!(reference.set_param(index, value));
                }
            }
            if block == 7 {
                for lane in prepared.take().unwrap() {
                    fixture
                        .commands
                        .push(EngineCommand::SetAutomationLane {
                            track_id: fixture.source,
                            lane,
                        })
                        .unwrap();
                }
            }
            let observed = check_callback(
                &mut fixture,
                reference.as_mut(),
                position,
                block >= 7,
                false,
                &mut levels,
            );
            for (_, _, _, meter) in observed.scopes {
                if windows == 0 {
                    assert_eq!(meter.first_source_frame, 896);
                    assert!(meter.transition_active);
                    assert_eq!(meter.blending, [u64::MAX; 2]);
                    assert!(meter.primary_acquisitions.iter().any(|&word| word != 0));
                    assert!(meter.secondary_acquisitions.iter().any(|&word| word != 0));
                } else {
                    assert!(!meter.transition_active);
                    assert!(meter
                        .output
                        .iter()
                        .zip(meter.input)
                        .all(|(&output, input)| output == input));
                }
                windows += 1;
            }
        }
        assert_eq!(windows, 2);
        for (index, value) in native(FINAL).into_iter().enumerate() {
            assert_eq!(
                fixture.engine.tracks[0].effects[0].effect.get_param(index),
                value
            );
        }
    }
}

#[test]
fn normalized_mix_step_is_exact_at_the_graph_control_boundary_and_fallback_callback_boundary() {
    for graph in [false, true] {
        let (mut fixture, mut reference, _) = prepare(graph, true);
        let mut levels = WindowLevels::default();
        let mut windows = 0;
        for block in 0..16 {
            let observed = check_callback(
                &mut fixture,
                reference.as_mut(),
                block * 128,
                true,
                true,
                &mut levels,
            );
            windows += observed.scopes.len();
        }
        assert_eq!(windows, 2);
        assert_eq!(fixture.engine.tracks[0].effects[0].effect.get_param(2), 1.0);
    }
}
