//! Short Delay control tooltips retain real applied movement and rate-limited cutoff information.

use super::delay_readouts::bank_tooltip;
use vibez_dsp::{delay::{param, DelayEffect}, effect::AudioEffect};

#[test]
fn bank_tooltip_tracks_live_glide_and_effective_cutoffs_without_a_graph() {
    let mut effect = DelayEffect::new(1000.0);
    for (index, value) in [
        (param::TIME_MS, 3.0), (param::RIGHT_TIME_MS, 5.0), (param::LINK, 0.0),
        (param::TIME_CHANGE, 1.0), (param::FEEDBACK_HP_ENABLED, 1.0),
        (param::FEEDBACK_HP_HZ, 1000.0), (param::FEEDBACK_LP_ENABLED, 1.0),
        (param::FEEDBACK_LP_HZ, 20000.0),
    ] {
        assert!(effect.set_param(index, value));
    }
    effect.process(&mut [0.0, 0.0], 2);
    assert!(effect.set_param(param::TIME_MS, 1000.0));
    effect.process(&mut [0.0, 0.0], 2);
    let first = effect.delay_snapshot().unwrap();
    assert!(first.lines[0].gliding);
    assert_eq!(first.lines[0].old_samples, 3.5);
    assert_eq!(first.lines[0].target_samples, 1000.0);
    assert_eq!(first.lines[0].remaining_frames, 1993);
    assert_eq!(first.filters[1].cutoff_hz, 450.0);
    assert_eq!(first.targets[param::FEEDBACK_LP_HZ], 20000.0);
    let tooltip = bank_tooltip(Some(&first));
    assert!(tooltip.contains("L pitch glide: applied 3.50 ms; target 1000.00 ms; 1.993 s remaining"));
    assert!(tooltip.contains("R settled: applied 5.00 ms; target 5.00 ms"));
    assert!(tooltip.contains("HP stored 1000.00 Hz; effective 450.00 Hz"));
    assert!(tooltip.contains("LP stored 20000.00 Hz; effective 450.00 Hz"));
    assert!(tooltip.contains("sample-rate limited"));
    assert!(tooltip.contains("Duck"));

    effect.process(&mut [0.0; 200], 2);
    let later = effect.delay_snapshot().unwrap();
    assert_eq!(later.lines[0].old_samples, 53.5);
    assert_eq!(later.lines[0].remaining_frames, 1893);
    let updated = bank_tooltip(Some(&later));
    assert!(updated.contains("L pitch glide: applied 53.50 ms; target 1000.00 ms; 1.893 s remaining"));
    assert!(!updated.contains("applied 3.50 ms"));
    assert_ne!(tooltip, updated);
    effect.reset();
    let unavailable = bank_tooltip(effect.delay_snapshot().as_ref());
    assert!(unavailable.contains("unavailable"));
    assert!(!unavailable.contains("450.00 Hz"));
    assert!(!unavailable.contains("53.50 ms"));
}
