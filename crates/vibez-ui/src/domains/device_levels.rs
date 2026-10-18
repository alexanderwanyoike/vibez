//! The levels a native device card shows, as the engine measures them.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use vibez_engine::metering::MeterValues;

/// A card's panel state that also shows measured input and output levels.
pub trait MeteredPanel {
    fn accept_levels(&mut self, input: MeterValues, output: MeterValues, now: Instant);
    /// Clears readings once audio stops arriving, so a held level never
    /// claims signal that is no longer there.
    fn reset_stale(&mut self, now: Instant);
    fn clear_readings(&mut self);
}

/// Readings arrive about once a frame, so this bounds a history whatever
/// its window.
pub const MAX_READINGS: usize = 600;

/// A device's recent readings, oldest first, for the histories its card plots.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadingHistory<T> {
    readings: VecDeque<(Instant, T)>,
}

impl<T> Default for ReadingHistory<T> {
    fn default() -> Self {
        Self {
            readings: VecDeque::new(),
        }
    }
}

impl<T> ReadingHistory<T> {
    /// Adds a reading and forgets those more than `window` older.
    pub fn push(&mut self, at: Instant, reading: T, window: Duration) {
        while self.readings.front().is_some_and(|(then, _)| {
            self.readings.len() >= MAX_READINGS || at.saturating_duration_since(*then) > window
        }) {
            self.readings.pop_front();
        }
        self.readings.push_back((at, reading));
    }

    pub fn clear(&mut self) {
        self.readings.clear();
    }

    pub fn len(&self) -> usize {
        self.readings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.readings.is_empty()
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &(Instant, T)> {
        self.readings.iter()
    }
}

impl<T> FromIterator<(Instant, T)> for ReadingHistory<T> {
    fn from_iter<I: IntoIterator<Item = (Instant, T)>>(readings: I) -> Self {
        Self {
            readings: readings.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_history_keeps_its_window_and_a_bounded_count() {
        let now = Instant::now();
        let window = Duration::from_secs(5);
        let mut history = ReadingHistory::default();
        for step in 0..800 {
            history.push(now + Duration::from_millis(step), step, window);
        }
        assert_eq!(history.len(), MAX_READINGS);
        assert_eq!(history.iter().next().unwrap().1, 200);
        history.push(now + Duration::from_secs(10), 0, window);
        assert_eq!(history.len(), 1, "older readings fall out of the window");
    }
}
