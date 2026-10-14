//! Independent scalar trajectories specify bounded manual transitions and exact automation.

use vibez_core::{effect::EffectType, id::EffectId};
use vibez_dsp::factory::create_effect_for_instance;

// Signed midpoint differences /2^24, interleaved L/R for identity17, source frames0..2047.
// Independent fixture SHA256: 8dc9ea018dfd7bf64792ca0102d27004aed5cbca1ec1982fffef6bb515853ec9.
const NOISE: &[u8; 16_384] = include_bytes!("fixtures/bitcrush_tpdf_identity17.bin");

fn noise(frame: usize, channel: usize) -> f64 {
    let start = (frame * 2 + channel) * 4;
    f64::from(i32::from_le_bytes(NOISE[start..start + 4].try_into().unwrap())) / 16_777_216.0
}

#[derive(Clone, Copy, PartialEq)]
struct Tuple {
    bits: u32,
    divider: u32,
    rounding: u32,
    dither: bool,
}

impl Tuple {
    fn from_raw(raw: [f32; 4]) -> Self {
        Self { bits: raw[0] as u32, divider: raw[1].round() as u32,
            rounding: raw[2] as u32, dither: raw[3] != 0.0 }
    }

    fn edit(&mut self, index: usize, value: f32) {
        match index {
            0 => self.bits = value as u32,
            1 => self.divider = value.round() as u32,
            4 => self.rounding = value as u32,
            5 => self.dither = value != 0.0,
            _ => panic!("not a discrete field"),
        }
    }
}

#[derive(Clone, Copy)]
struct Lane {
    tuple: Tuple,
    counter: u32,
    held: [f32; 2],
}

impl Lane {
    fn step(&mut self, frame: usize) -> ([f32; 2], bool) {
        let acquired = self.counter == 0;
        if acquired {
            let quantum = 2.0_f32 / (1_u32 << self.tuple.bits) as f32;
            for channel in 0..2 {
                let input = input(frame)[channel];
                let scaled = if self.tuple.dither {
                    f64::from(input) / f64::from(quantum) + noise(frame, channel)
                } else {
                    f64::from(input / quantum)
                };
                let code = match self.tuple.rounding {
                    1 => scaled.floor(),
                    2 => scaled.trunc(),
                    _ => scaled.round(),
                };
                self.held[channel] = (code * f64::from(quantum)).clamp(-1.0, 1.0) as f32;
            }
        }
        self.counter = (self.counter + 1) % self.tuple.divider;
        (self.held, acquired)
    }
}

struct Reference {
    incumbent: Lane,
    incoming: Option<Lane>,
    latest: Tuple,
    progress: u32,
}

struct Frame {
    output: [f32; 2],
    primary: bool,
    secondary: bool,
    transition: bool,
    end_primary_interval: u32,
    end_secondary_interval: u32,
    end_transition: bool,
}

impl Reference {
    fn edit(&mut self, edit: &Edit) {
        self.latest.edit(edit.index, edit.value);
        if edit.automated {
            self.incumbent.tuple.edit(edit.index, edit.value);
            if let Some(incoming) = &mut self.incoming {
                incoming.tuple.edit(edit.index, edit.value);
            }
        }
    }

    fn step(&mut self, frame: usize) -> Frame {
        if self.incoming.is_none() && self.latest != self.incumbent.tuple {
            let mut incoming = self.incumbent;
            incoming.tuple = self.latest;
            self.incoming = Some(incoming);
            self.progress = 0;
        }
        let (outgoing, primary) = self.incumbent.step(frame);
        let Some(incoming) = &mut self.incoming else {
            return Frame { output: outgoing, primary, secondary: false, transition: false,
                end_primary_interval: self.incumbent.tuple.divider,
                end_secondary_interval: 0, end_transition: false };
        };
        let (incoming_output, secondary) = incoming.step(frame);
        self.progress += 1;
        let weight = self.progress as f32 / 512.0;
        let output = std::array::from_fn(|channel| outgoing[channel] * (1.0 - weight)
            + incoming_output[channel] * weight);
        if self.progress == 512 {
            self.incumbent = self.incoming.take().unwrap();
        }
        Frame { output, primary, secondary, transition: true,
            end_primary_interval: self.incumbent.tuple.divider,
            end_secondary_interval: self.incoming.map_or(0, |lane| lane.tuple.divider),
            end_transition: self.incoming.is_some() }
    }
}

#[derive(Clone, Copy)]
struct Edit {
    frame: usize,
    index: usize,
    value: f32,
    automated: bool,
}

fn manual(frame: usize, index: usize, value: f32) -> Edit {
    Edit { frame, index, value, automated: false }
}

fn automated(frame: usize, index: usize, value: f32) -> Edit {
    Edit { frame, index, value, automated: true }
}

fn input(frame: usize) -> [f32; 2] {
    let left = ((frame % 137) as f32 - 68.0) / 97.0;
    [left, 0.173 - left * 0.37]
}

fn check(initial: [f32; 4], frames: usize, edits: &[Edit]) -> Vec<Frame> {
    let tuple = Tuple::from_raw(initial);
    let mut reference = Reference { incumbent: Lane { tuple, counter: 0, held: [0.0; 2] },
        incoming: None, latest: tuple, progress: 0 };
    let expected: Vec<_> = (0..frames).map(|frame| {
        for edit in edits.iter().filter(|edit| edit.frame == frame) {
            reference.edit(edit);
        }
        reference.step(frame)
    }).collect();
    for channels in [1, 2] {
        let mut baseline = None;
        for partitions in [&[1][..], &[127, 7, 31, 5][..]] {
            let mut effect = create_effect_for_instance(
                EffectType::Bitcrush, 48_000.0,
                &[initial[0], initial[1], 1.0, 0.0, initial[2], initial[3], 0.0, 0.0, 12_000.0],
                serde_json::from_str::<EffectId>("17").unwrap(),
            );
            let mut rendered = Vec::with_capacity(frames * channels);
            let mut position = 0;
            let mut callback = 0;
            while position < frames {
                for edit in edits.iter().filter(|edit| edit.frame == position) {
                    assert!(if edit.automated {
                        effect.set_automated_param(edit.index, edit.value)
                    } else {
                        effect.set_param(edit.index, edit.value)
                    });
                    assert_eq!(effect.get_param(edit.index), edit.value, "raw target must survive");
                }
                let next_edit = edits.iter().find(|edit| edit.frame > position).map_or(frames, |edit| edit.frame);
                let end = (position + partitions[callback % partitions.len()]).min(next_edit).min(frames);
                let mut audio: Vec<_> = (position..end)
                    .flat_map(|frame| input(frame).into_iter().take(channels)).collect();
                effect.process(&mut audio, channels);
                let meter = effect.take_sample_hold_meter().unwrap();
                assert_eq!(usize::from(meter.count), end - position);
                assert_eq!(meter.first_source_frame, position as u64);
                assert_eq!(meter.primary_interval_frames, expected[end - 1].end_primary_interval,
                    "packet end primary at frame{}", end - 1);
                assert_eq!(meter.secondary_interval_frames, expected[end - 1].end_secondary_interval,
                    "packet end secondary at frame{}", end - 1);
                assert_eq!(meter.transition_active, expected[end - 1].end_transition,
                    "packet end transition at frame{}", end - 1);
                for (point, frame) in (position..end).enumerate() {
                    let bit = 1_u64 << (point % 64);
                    let word = point / 64;
                    assert_eq!(meter.primary_acquisitions[word] & bit != 0, expected[frame].primary,
                        "primary acquisition at frame{frame}");
                    assert_eq!(meter.secondary_acquisitions[word] & bit != 0, expected[frame].secondary,
                        "secondary acquisition at frame{frame}");
                    assert_eq!(meter.blending[word] & bit != 0, expected[frame].transition,
                        "historical transition at frame{frame}");
                    assert_eq!(meter.input[point], input(frame)[0]);
                    assert_eq!(meter.output[point], audio[point * channels]);
                    for channel in 0..channels {
                        assert_eq!(audio[point * channels + channel], expected[frame].output[channel],
                            "trajectory at frame{frame}, channel={channel}, partitions={partitions:?}");
                    }
                }
                rendered.extend(audio);
                position = end;
                callback += 1;
            }
            if let Some(baseline) = &baseline {
                assert_eq!(&rendered, baseline, "callback partition invariance");
            } else {
                baseline = Some(rendered);
            }
        }
    }
    expected
}

#[test]
fn a_batched_tuple_fades_exact_old_and_new_trajectories_through_the_endpoint() {
    let edits = [manual(16, 0, 4.0), manual(16, 1, 5.0), manual(16, 4, 1.0), manual(16, 5, 1.0),
        manual(100, 0, 4.75), manual(100, 1, 5.1)];
    let expected = check([16.0, 3.0, 0.0, 0.0], 600, &edits);
    assert!(expected[526].transition && expected[526].end_transition);
    assert!(expected[527].transition && !expected[527].end_transition);
    assert_eq!(expected[527].end_secondary_interval, 0);
    assert_eq!(expected[527].end_primary_interval, 5);
    assert!(!expected[528].transition && !expected[528].end_transition);
}

#[test]
fn pending_reversals_and_own_field_automation_do_not_revive_superseded_intent() {
    let edits = [
        manual(16, 0, 6.0), manual(16, 1, 3.0), manual(16, 4, 1.0), manual(16, 5, 1.0),
        manual(128, 0, 3.0), manual(128, 1, 11.0), manual(128, 4, 2.0), manual(128, 5, 0.0),
        automated(300, 0, 9.0), automated(300, 1, 5.0), automated(300, 5, 1.0),
        manual(400, 4, 0.0), manual(527, 4, 2.0), automated(600, 4, 0.0),
        manual(1038, 1, 8.0), manual(1039, 0, 5.0), manual(1040, 1, 8.0),
    ];
    let expected = check([12.0, 7.0, 0.0, 0.0], 1600, &edits);
    for endpoint in [527, 1039, 1551] {
        assert!(expected[endpoint - 1].end_transition);
        assert!(expected[endpoint].transition && !expected[endpoint].end_transition);
    }
    assert!(expected[528].transition && expected[1040].transition);
    assert!(!expected[1552].transition);
}

#[test]
fn every_frame_retargets_coalesce_and_settle_within_the_frozen_bound() {
    let mut edits = Vec::new();
    for frame in 32..=700 {
        edits.extend([manual(frame, 0, (2 + frame % 15) as f32),
            manual(frame, 1, (1 + frame % 32) as f32), manual(frame, 4, (frame % 3) as f32),
            manual(frame, 5, (frame % 2) as f32)]);
    }
    edits.extend([manual(1600, 0, 12.5), manual(1600, 1, 29.1)]);
    let expected = check([16.0, 7.0, 0.0, 0.0], 1730, &edits);
    assert!(expected[1567].transition && !expected[1567].end_transition);
    assert!(expected[1568..].iter().all(|frame| !frame.transition));
    let settled = expected.iter().rposition(|frame| frame.transition).unwrap() + 1;
    assert_eq!(settled, 1568);
    assert!(settled - 700 <= 1024);
}

#[test]
fn effective_fractional_noops_preserve_raw_targets_and_clock_history() {
    let edits = [manual(16, 0, 10.99), manual(16, 1, 4.1), automated(32, 0, 10.75),
        automated(32, 1, 4.49), manual(64, 4, 0.0), manual(64, 5, 0.0)];
    let expected = check([10.49, 4.49, 0.0, 0.0], 200, &edits);
    assert!(expected.iter().all(|frame| !frame.transition));
}
