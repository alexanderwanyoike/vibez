//! Requires the appended Delay parameter layout to preserve legacy units and canonical retained targets.

use serde_json::json;
use vibez_dsp::{delay::DelayEffect, effect::AudioEffect};

const PARAMETERS: [(&str, f32, f32, f32, &str); 17] = [
    ("Time", 1.0, 2000.0, 500.0, "ms"),
    ("Feedback", 0.0, 0.95, 0.5, ""),
    ("Mix", 0.0, 1.0, 0.3, ""),
    ("Sync", 0.0, 1.0, 0.0, ""),
    ("Left division", 0.0, 6.0, 4.0, ""),
    ("Left modifier", 0.0, 2.0, 0.0, ""),
    ("Routing", 0.0, 1.0, 0.0, ""),
    ("L/R Link", 0.0, 1.0, 1.0, ""),
    ("Right Free Time", 1.0, 2000.0, 500.0, "ms"),
    ("Right division", 0.0, 6.0, 4.0, ""),
    ("Right modifier", 0.0, 2.0, 0.0, ""),
    ("Feedback HP enable", 0.0, 1.0, 0.0, ""),
    ("Feedback HP frequency", 20.0, 1000.0, 120.0, "Hz"),
    ("Feedback LP enable", 0.0, 1.0, 0.0, ""),
    ("Feedback LP frequency", 1000.0, 20000.0, 8000.0, "Hz"),
    ("Duck", 0.0, 1.0, 0.0, ""),
    ("Time change", 0.0, 1.0, 0.0, ""),
];

const RETAINED: [f32; 17] = [
    531.25, 0.625, 0.375, 1.0, 2.0, 1.0, 1.0, 0.0, 987.5, 6.0, 2.0, 1.0, 220.5, 1.0, 12000.5, 0.25,
    1.0,
];

fn targets(effect: &DelayEffect) -> [u32; 17] {
    std::array::from_fn(|index| effect.get_param(index).to_bits())
}

fn edit(effect: &mut DelayEffect, automated: bool, index: usize, value: f32) -> bool {
    if automated {
        effect.set_automated_param(index, value)
    } else {
        effect.set_param(index, value)
    }
}

fn record(case: &str, observed: serde_json::Value) {
    println!(
        "DELAY_PARAMETER_LAYOUT_RECORD {}",
        json!({
            "case": case,
            "execution_head": std::env::var("VIBEZ_VERIFICATION_HEAD")
                .unwrap_or_else(|_| "UNLABELLED".into()),
            "observed": observed,
        })
    );
}

#[test]
fn seventeen_descriptors_append_neutral_defaults_and_preserve_legacy_normalized_meaning() {
    let mut effect = DelayEffect::new(1000.0);
    let mut failures = Vec::new();
    let mut descriptors = Vec::new();
    if effect.param_descriptors().len() != 17 {
        failures.push(format!(
            "descriptor count={} expected17",
            effect.param_descriptors().len()
        ));
    }
    for (index, (meaning, min, max, default, unit)) in PARAMETERS.into_iter().enumerate() {
        let actual = effect.param_descriptors().get(index).copied();
        let matches = actual.is_some_and(|descriptor| {
            (if index < 3 {
                descriptor.name == meaning
            } else {
                !descriptor.name.is_empty()
            }) && descriptor.min.to_bits() == min.to_bits()
                && descriptor.max.to_bits() == max.to_bits()
                && descriptor.default.to_bits() == default.to_bits()
                && descriptor.unit == unit
        }) && effect.get_param(index).to_bits() == default.to_bits();
        if !matches {
            failures.push(format!(
                "descriptor/default index={index} ({meaning}) mismatch"
            ));
        }
        descriptors.push(json!({
            "index": index, "meaning": meaning, "matches": matches,
            "expected_bits": [min.to_bits(), max.to_bits(), default.to_bits()], "expected_unit": unit,
            "actual_name": actual.map(|descriptor| descriptor.name),
            "actual_bits": actual.map(|descriptor| [descriptor.min.to_bits(), descriptor.max.to_bits(), descriptor.default.to_bits()]),
            "actual_unit": actual.map(|descriptor| descriptor.unit),
            "getter_bits": effect.get_param(index).to_bits(),
        }));
    }
    let normalized_cases: [(usize, [f32; 5]); 3] = [
        (0, [1.0, 500.75, 1000.5, 1500.25, 2000.0]),
        (1, [0.0, 0.2375, 0.475, 0.7125, 0.95]),
        (2, [0.0, 0.25, 0.5, 0.75, 1.0]),
    ];
    let mut normalized = Vec::new();
    for (index, expected) in normalized_cases {
        for (value, expected) in [0.0, 0.25, 0.5, 0.75, 1.0].into_iter().zip(expected) {
            let descriptor = effect.param_descriptors()[index];
            let stored = descriptor.min + value * (descriptor.max - descriptor.min);
            let accepted = effect.set_automated_param(index, stored);
            let actual = effect.get_param(index);
            if !accepted || actual.to_bits() != expected.to_bits() {
                failures.push(format!(
                    "legacy normalized index={index}, value={value} changed meaning"
                ));
            }
            normalized.push(json!({
                "index": index, "normalized": value, "accepted": accepted,
                "expected_stored_bits": expected.to_bits(), "actual_stored_bits": actual.to_bits(),
            }));
        }
    }
    record(
        "layout_defaults_and_legacy_descriptor_mapping",
        json!({
            "descriptor_count": effect.param_descriptors().len(), "descriptors": descriptors,
            "legacy_mapping_scope": "Public descriptors and setter only; actual engine ingress remains a separate gate",
            "legacy_normalized_mapping": normalized,
        }),
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn finite_manual_and_automated_targets_are_canonical_and_preserve_other_retained_values() {
    let mut cases: Vec<(usize, f32, f32)> = Vec::new();
    for index in [3, 7, 11, 13] {
        for (input, expected) in [
            (-f32::MAX, 0.0),
            (-0.0, 0.0),
            (0.499_999_97, 0.0),
            (0.5, 1.0),
            (0.500_000_06, 1.0),
            (f32::MAX, 1.0),
        ] {
            cases.push((index, input, expected));
        }
    }
    for (index, expected) in [
        (4, [0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 3.0, 6.0, 6.0]),
        (5, [0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 2.0, 2.0]),
        (6, [0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0]),
        (9, [0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 3.0, 6.0, 6.0]),
        (10, [0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 2.0, 2.0]),
        (16, [0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0]),
    ] {
        for (input, expected) in [-f32::MAX, -0.25, 0.49, 0.5, 1.49, 1.5, 2.5, 5.5, f32::MAX]
            .into_iter()
            .zip(expected)
        {
            cases.push((index, input, expected));
        }
    }
    for (index, cases_for_index) in [
        (
            8,
            [
                (-f32::MAX, 1.0),
                (0.0, 1.0),
                (531.25, 531.25),
                (f32::MAX, 2000.0),
            ],
        ),
        (
            12,
            [
                (-f32::MAX, 20.0),
                (20.0, 20.0),
                (220.5, 220.5),
                (f32::MAX, 1000.0),
            ],
        ),
        (
            14,
            [
                (-f32::MAX, 1000.0),
                (1000.0, 1000.0),
                (12000.5, 12000.5),
                (f32::MAX, 20000.0),
            ],
        ),
        (
            15,
            [
                (-f32::MAX, 0.0),
                (0.0, 0.0),
                (0.375, 0.375),
                (f32::MAX, 1.0),
            ],
        ),
    ] {
        for (input, expected) in cases_for_index {
            cases.push((index, input, expected));
        }
    }
    let mut failures = Vec::new();
    let mut observations = Vec::new();
    for automated in [false, true] {
        let mut effect = DelayEffect::new(1000.0);
        let prepared: Vec<_> = RETAINED
            .into_iter()
            .enumerate()
            .map(|(index, value)| edit(&mut effect, automated, index, value))
            .collect();
        if !prepared.iter().all(|accepted| *accepted)
            || targets(&effect) != RETAINED.map(f32::to_bits)
        {
            failures.push(format!(
                "automated={automated}: full retained vector not stored"
            ));
        }
        for &(index, input, expected) in &cases {
            let before = targets(&effect);
            let mut expected_targets = before;
            expected_targets[index] = expected.to_bits();
            let accepted = edit(&mut effect, automated, index, input);
            let actual = targets(&effect);
            let repeated = edit(&mut effect, automated, index, input);
            let repeated_targets = targets(&effect);
            if !accepted
                || !repeated
                || actual != expected_targets
                || repeated_targets != expected_targets
            {
                failures.push(format!("automated={automated}, index={index}, input={input}: canonical/retained target mismatch"));
            }
            observations.push(json!({
                "automated": automated, "index": index, "requested_bits": input.to_bits(),
                "accepted": accepted, "repeated_accepted": repeated,
                "before_target_bits": before, "expected_target_bits": expected_targets,
                "actual_target_bits": actual, "repeated_target_bits": repeated_targets,
            }));
        }
        let before_reset = targets(&effect);
        effect.reset();
        if targets(&effect) != before_reset {
            failures.push(format!("automated={automated}: reset rewrote targets"));
        }
    }
    record(
        "finite_canonical_targets_and_retention",
        json!(observations),
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn nonfinite_and_unknown_edits_reject_without_mutating_any_of_seventeen_targets() {
    let mut failures = Vec::new();
    let mut observations = Vec::new();
    for automated in [false, true] {
        let mut effect = DelayEffect::new(1000.0);
        let prepared: Vec<_> = RETAINED
            .into_iter()
            .enumerate()
            .map(|(index, value)| edit(&mut effect, automated, index, value))
            .collect();
        if !prepared.iter().all(|accepted| *accepted)
            || targets(&effect) != RETAINED.map(f32::to_bits)
        {
            failures.push(format!(
                "automated={automated}: full rejection baseline not stored"
            ));
        }
        let mut cases = Vec::new();
        for index in 0..17 {
            for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                cases.push((index, value));
            }
        }
        for index in [17, 18, usize::MAX] {
            for value in [0.25, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                cases.push((index, value));
            }
        }
        for (index, value) in cases {
            let before = targets(&effect);
            let accepted = edit(&mut effect, automated, index, value);
            let after = targets(&effect);
            if accepted || before != after {
                failures.push(format!(
                    "automated={automated}, index={index}, bits={}: rejected edit mutated targets",
                    value.to_bits()
                ));
            }
            observations.push(json!({
                "automated": automated, "index": index, "requested_bits": value.to_bits(),
                "accepted": accepted, "before_target_bits": before, "after_target_bits": after,
            }));
        }
    }
    record("full_layout_rejection_is_atomic", json!(observations));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
