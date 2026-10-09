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
