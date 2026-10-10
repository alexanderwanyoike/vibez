//! Cold and seek history preserves continuous delayed musical coordinates.

use super::ChannelClock;
use vibez_core::id::TrackId;

#[test]
fn cold_history_extrapolates_from_the_source_origin_across_callbacks_and_seeks() {
    let mut clock = ChannelClock::prepare(TrackId::new(), 1000, 256).unwrap();
    for start in [10000u64, 500, 20000] {
        clock.clear();
        for block in 0..8 {
            let block_start = block * 256;
            clock.record(start + block_start, 256, true);
            for offset in 0..256 {
                let physical = block_start + offset as u64;
                assert_eq!(
                    clock.position(1000, offset),
                    (start + physical).saturating_sub(1000),
                    "start {start}, block {block}, offset {offset}"
                );
                assert_eq!(clock.has_context(1000, offset), physical >= 1000);
            }
        }
    }
}

#[test]
fn an_early_loop_wrap_does_not_rebase_unfilled_history_to_the_current_block() {
    let source_position = |physical: u64| {
        if physical < 768 {
            10000 + physical
        } else {
            (physical - 768) % 256
        }
    };
    let mut clock = ChannelClock::prepare(TrackId::new(), 1000, 256).unwrap();
    for block in 0..10 {
        let block_start = block * 256;
        clock.record(source_position(block_start), 256, true);
        for offset in 0..256 {
            let physical = block_start + offset as u64;
            let expected = if physical < 1000 {
                10000 - (1000 - physical)
            } else {
                source_position(physical - 1000)
            };
            assert_eq!(
                clock.position(1000, offset),
                expected,
                "block {block}, offset {offset}"
            );
            assert_eq!(clock.has_context(1000, offset), physical >= 1000);
        }
    }
}

#[test]
fn a_delay_beyond_retained_capacity_never_reads_an_overwritten_clock_slot() {
    let mut clock = ChannelClock::prepare(TrackId::new(), 2, 2).unwrap();
    for block in 0..5 {
        clock.record(block * 2, 2, true);
    }
    assert_eq!(clock.before_block(1), Some(9));
    assert_eq!(clock.before_block(5), Some(5));
    assert_eq!(clock.before_block(6), None);
    assert!(!clock.has_context(100, 0));
}

#[test]
fn empty_command_drain_has_no_recorded_audio_context() {
    let mut clock = ChannelClock::prepare(TrackId::new(), 0, 16).unwrap();
    clock.record(100, 0, true);
    assert!(!clock.has_context(0, 0));
    assert_eq!(clock.position(0, 0), 0);
    clock.record(100, 16, true);
    clock.record(116, 0, true);
    assert_eq!(clock.before_block(1), Some(115));
    assert!(!clock.has_context(0, 0));
    assert_eq!(clock.position(0, 0), 0);
}

#[test]
fn automation_preparation_checks_values_and_delay_together_before_allocation() {
    use super::PreparedAutomationControl;
    use vibez_core::automation::AutomationTarget;
    assert!(PreparedAutomationControl::prepare(
        TrackId::new(),
        AutomationTarget::TrackGain,
        0,
        3,
        8,
        10
    )
    .is_err());
    assert_eq!(
        PreparedAutomationControl::prepare(
            TrackId::new(),
            AutomationTarget::TrackGain,
            0,
            3,
            8,
            11
        )
        .unwrap()
        .storage_samples(),
        11
    );
    assert!(PreparedAutomationControl::required_samples(3, usize::MAX).is_err());
}

#[test]
fn context_segments_keep_held_clocks_whole_and_split_at_a_recorded_wrap() {
    let mut clock = ChannelClock::prepare(TrackId::new(), 8, 16).unwrap();
    clock.record(100, 8, false);
    assert_eq!(clock.segment_end(0, 0, 8), 8);
    clock.record(200, 8, true);
    assert_eq!(clock.segment_end(0, 0, 8), 8);
    clock.record(0, 8, true);
    assert_eq!(clock.segment_end(4, 0, 8), 4);
    assert_eq!(clock.segment_end(4, 4, 8), 8);
}
