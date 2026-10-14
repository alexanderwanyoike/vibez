//! Shared clock acquisition and interval telemetry follow an independent tag-zero oracle.

use vibez_core::{effect::EffectType, id::EffectId};
use vibez_dsp::factory::create_effect_for_instance;

struct ClockCase {
    name: &'static str,
    frames: usize,
    divider: f32,
    jitter: f32,
    edits: &'static [(usize, usize, f32, bool)],
    events: &'static [(usize, bool, u32)],
}

fn input(frame: usize) -> [f32; 2] {
    let left = ((frame % 257) as f32 - 128.0) / 512.0;
    [left, 0.125 - left * 0.5]
}

fn check_clock(case: &ClockCase) {
    let mut expected = Vec::with_capacity(case.frames);
    let mut held = [0.0; 2];
    let mut interval = 0;
    for frame in 0..case.frames {
        let event = case.events.iter().find(|event| event.0 == frame);
        let acquired = event.is_some_and(|event| event.1);
        if let Some(event) = event {
            interval = event.2;
        }
        if acquired {
            held = input(frame);
        }
        expected.push((held, acquired, interval));
    }
    let mut baseline = None;
    for dither in [0.0, 1.0] {
        for partitions in [&[128][..], &[1, 7, 31, 5, 63][..]] {
            let mut effect = create_effect_for_instance(
                EffectType::Bitcrush,
                48_000.0,
                &[
                    16.0,
                    case.divider,
                    1.0,
                    0.0,
                    0.0,
                    dither,
                    case.jitter,
                    0.0,
                    12_000.0,
                ],
                serde_json::from_str::<EffectId>("17").unwrap(),
            );
            let mut rendered = Vec::with_capacity(case.frames * 2);
            let mut position = 0;
            let mut callback = 0;
            while position < case.frames {
                for &(_, parameter, value, automated) in
                    case.edits.iter().filter(|edit| edit.0 == position)
                {
                    assert!(if automated {
                        effect.set_automated_param(parameter, value)
                    } else {
                        effect.set_param(parameter, value)
                    });
                }
                let next_edit = case
                    .edits
                    .iter()
                    .find(|edit| edit.0 > position)
                    .map_or(case.frames, |edit| edit.0);
                let end = (position + partitions[callback % partitions.len()])
                    .min(next_edit)
                    .min(case.frames);
                let mut audio: Vec<_> = (position..end).flat_map(input).collect();
                effect.process(&mut audio, 2);
                let meter = effect.take_sample_hold_meter().unwrap();
                assert_eq!(usize::from(meter.count), end - position);
                assert_eq!(meter.first_source_frame, position as u64);
                assert_eq!(meter.sample_rate, 48_000.0);
                assert_eq!(meter.primary_interval_frames, expected[end - 1].2,
                    "{}: actual latched interval at frame{}, Dither={dither}, partitions={partitions:?}", case.name, end - 1);
                assert_eq!(meter.secondary_interval_frames, 0);
                assert!(!meter.transition_active);
                assert_eq!(meter.secondary_acquisitions, [0; 2]);
                assert_eq!(meter.blending, [0; 2]);
                for (point, frame) in (position..end).enumerate() {
                    let acquired =
                        meter.primary_acquisitions[point / 64] & (1_u64 << (point % 64)) != 0;
                    assert_eq!(
                        acquired, expected[frame].1,
                        "{}: acquisition frame{frame}, Dither={dither}, partitions={partitions:?}",
                        case.name
                    );
                    assert_eq!(meter.input[point], input(frame)[0]);
                    assert_eq!(meter.output[point], audio[point * 2]);
                    if dither == 0.0 {
                        assert_eq!(
                            &audio[point * 2..point * 2 + 2],
                            &expected[frame].0,
                            "{}: held stereo samples at frame{frame}, partitions={partitions:?}",
                            case.name
                        );
                    }
                }
                rendered.extend(audio);
                position = end;
                callback += 1;
            }
            if dither == 0.0 {
                if let Some(baseline) = &baseline {
                    assert_eq!(
                        &rendered, baseline,
                        "{}: callback partition invariance",
                        case.name
                    );
                } else {
                    baseline = Some(rendered);
                }
            }
        }
    }
}

#[test]
fn fixed_jitter_and_divider_phase_matches_independent_acquisitions_and_latched_intervals() {
    // Frozen scalar oracle: jitter-clock-golden.py SHA256 9ceeacd0c7f900c392b37bfb2e32d1126be235c6151e31bb6f4f8896db3d9841.
    check_clock(&ClockCase {
        name: "fixed_jitter_and_divider_phase",
        frames: 200,
        divider: 7.0,
        jitter: 0.75,
        edits: &[
            (19, 1, 11.0, true),
            (42, 1, 3.0, true),
            (77, 1, 1.0, true),
            (105, 1, 32.0, true),
            (143, 1, 5.0, true),
        ],
        events: &[
            (0, true, 8),
            (8, true, 9),
            (17, true, 6),
            (19, false, 10),
            (27, true, 11),
            (38, true, 12),
            (42, false, 3),
            (44, true, 2),
            (46, true, 3),
            (49, true, 3),
            (52, true, 3),
            (55, true, 4),
            (59, true, 3),
            (62, true, 4),
            (66, true, 2),
            (68, true, 3),
            (71, true, 3),
            (74, true, 3),
            (77, true, 1),
            (78, true, 1),
            (79, true, 1),
            (80, true, 1),
            (81, true, 1),
            (82, true, 1),
            (83, true, 1),
            (84, true, 1),
            (85, true, 1),
            (86, true, 1),
            (87, true, 1),
            (88, true, 1),
            (89, true, 1),
            (90, true, 1),
            (91, true, 1),
            (92, true, 1),
            (93, true, 1),
            (94, true, 1),
            (95, true, 1),
            (96, true, 1),
            (97, true, 1),
            (98, true, 1),
            (99, true, 1),
            (100, true, 1),
            (101, true, 1),
            (102, true, 1),
            (103, true, 1),
            (104, true, 1),
            (105, true, 26),
            (131, true, 41),
            (143, false, 6),
            (149, true, 4),
            (153, true, 4),
            (157, true, 5),
            (162, true, 6),
            (168, true, 4),
            (172, true, 4),
            (176, true, 5),
            (181, true, 3),
            (184, true, 6),
            (190, true, 6),
            (196, true, 3),
            (199, true, 3),
        ],
    });
}

#[test]
fn manual_ramp_and_exact_automation_matches_independent_acquisitions_and_latched_intervals() {
    // Frozen scalar oracle: jitter-clock-golden.py SHA256 9ceeacd0c7f900c392b37bfb2e32d1126be235c6151e31bb6f4f8896db3d9841.
    check_clock(&ClockCase {
        name: "manual_ramp_and_exact_automation",
        frames: 900,
        divider: 13.0,
        jitter: 0.0,
        edits: &[
            (16, 6, 1.0, false),
            (129, 6, 1.0, false),
            (600, 6, 0.25, true),
            (640, 6, 0.75, false),
            (800, 6, 0.875, true),
        ],
        events: &[
            (0, true, 13),
            (13, true, 13),
            (26, true, 13),
            (39, true, 13),
            (52, true, 13),
            (65, true, 13),
            (78, true, 13),
            (91, true, 13),
            (104, true, 12),
            (116, true, 12),
            (128, true, 13),
            (141, true, 15),
            (156, true, 11),
            (167, true, 15),
            (182, true, 13),
            (195, true, 13),
            (208, true, 12),
            (220, true, 14),
            (234, true, 16),
            (250, true, 12),
            (262, true, 15),
            (277, true, 13),
            (290, true, 15),
            (305, true, 10),
            (315, true, 15),
            (330, true, 17),
            (347, true, 14),
            (361, true, 16),
            (377, true, 17),
            (394, true, 16),
            (410, true, 11),
            (421, true, 13),
            (434, true, 18),
            (452, true, 10),
            (462, true, 19),
            (481, true, 14),
            (495, true, 15),
            (510, true, 18),
            (528, true, 15),
            (543, true, 12),
            (555, true, 12),
            (567, true, 18),
            (585, true, 17),
            (602, true, 13),
            (615, true, 13),
            (628, true, 13),
            (641, true, 12),
            (653, true, 12),
            (665, true, 14),
            (679, true, 11),
            (690, true, 11),
            (701, true, 14),
            (715, true, 15),
            (730, true, 11),
            (741, true, 12),
            (753, true, 15),
            (768, true, 12),
            (780, true, 13),
            (793, true, 11),
            (804, true, 16),
            (820, true, 18),
            (838, true, 15),
            (853, true, 10),
            (863, true, 18),
            (881, true, 19),
        ],
    });
}

#[test]
fn zero_jitter_legacy_divider_phase_matches_independent_acquisitions_and_latched_intervals() {
    // Frozen scalar oracle: jitter-clock-golden.py SHA256 9ceeacd0c7f900c392b37bfb2e32d1126be235c6151e31bb6f4f8896db3d9841.
    check_clock(&ClockCase {
        name: "zero_jitter_legacy_divider_phase",
        frames: 200,
        divider: 7.0,
        jitter: 0.0,
        edits: &[
            (19, 1, 11.0, true),
            (42, 1, 3.0, true),
            (77, 1, 1.0, true),
            (105, 1, 32.0, true),
            (143, 1, 5.0, true),
        ],
        events: &[
            (0, true, 7),
            (7, true, 7),
            (14, true, 7),
            (19, false, 11),
            (25, true, 11),
            (36, true, 11),
            (42, false, 3),
            (45, true, 3),
            (48, true, 3),
            (51, true, 3),
            (54, true, 3),
            (57, true, 3),
            (60, true, 3),
            (63, true, 3),
            (66, true, 3),
            (69, true, 3),
            (72, true, 3),
            (75, true, 3),
            (77, false, 1),
            (78, true, 1),
            (79, true, 1),
            (80, true, 1),
            (81, true, 1),
            (82, true, 1),
            (83, true, 1),
            (84, true, 1),
            (85, true, 1),
            (86, true, 1),
            (87, true, 1),
            (88, true, 1),
            (89, true, 1),
            (90, true, 1),
            (91, true, 1),
            (92, true, 1),
            (93, true, 1),
            (94, true, 1),
            (95, true, 1),
            (96, true, 1),
            (97, true, 1),
            (98, true, 1),
            (99, true, 1),
            (100, true, 1),
            (101, true, 1),
            (102, true, 1),
            (103, true, 1),
            (104, true, 1),
            (105, true, 32),
            (137, true, 32),
            (143, false, 5),
            (147, true, 5),
            (152, true, 5),
            (157, true, 5),
            (162, true, 5),
            (167, true, 5),
            (172, true, 5),
            (177, true, 5),
            (182, true, 5),
            (187, true, 5),
            (192, true, 5),
            (197, true, 5),
        ],
    });
}

#[test]
fn n_one_with_full_jitter_still_acquires_every_frame_and_reports_one() {
    let mut effect = create_effect_for_instance(
        EffectType::Bitcrush,
        48_000.0,
        &[16.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 12_000.0],
        serde_json::from_str::<EffectId>("17").unwrap(),
    );
    let original: Vec<_> = (0..128).flat_map(input).collect();
    let mut audio = original.clone();
    effect.process(&mut audio, 2);
    assert_eq!(audio, original);
    let meter = effect.take_sample_hold_meter().unwrap();
    assert_eq!(meter.count, 128);
    assert_eq!(meter.primary_interval_frames, 1);
    assert_eq!(meter.primary_acquisitions, [u64::MAX; 2]);
}
