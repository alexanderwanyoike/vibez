//! The levels a native device card shows, as the engine measures them.

use std::time::Instant;

use vibez_engine::metering::MeterValues;

/// A card's panel state that also shows measured input and output levels.
pub trait MeteredPanel {
    fn accept_levels(&mut self, input: MeterValues, output: MeterValues, now: Instant);
    /// Clears readings once audio stops arriving, so a held level never
    /// claims signal that is no longer there.
    fn reset_stale(&mut self, now: Instant);
    fn clear_readings(&mut self);
}
