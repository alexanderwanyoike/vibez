//! Arcs handed to the audio engine, kept alive on the UI thread so the engine never drops the last owner.

use std::sync::Arc;

/// Dropping the final owner inside the audio callback would free memory on
/// the audio thread, so the UI keeps the one the engine plays now and the
/// last `N` it replaced, which the engine may still hold (a fading voice, or
/// a command it has not drained yet).
#[derive(Debug, Clone)]
pub struct EngineHeld<T, const N: usize> {
    current: Option<Arc<T>>,
    retired: [Option<Arc<T>>; N],
}

impl<T, const N: usize> Default for EngineHeld<T, N> {
    fn default() -> Self {
        Self {
            current: None,
            retired: std::array::from_fn(|_| None),
        }
    }
}

impl<T, const N: usize> EngineHeld<T, N> {
    /// Keeps `next`, the one the engine plays now, and retires the one before.
    pub fn hand(&mut self, next: Arc<T>) {
        self.retire();
        self.current = Some(next);
    }

    /// The engine no longer plays the current one, though it may still hold it.
    pub fn retire(&mut self) {
        if let Some(previous) = self.current.take() {
            if N > 0 {
                self.retired.rotate_right(1);
                self.retired[0] = Some(previous);
            }
        }
    }

    pub fn current(&self) -> Option<&Arc<T>> {
        self.current.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_current_arc_and_the_last_ones_it_replaced() {
        let arcs: Vec<_> = (0..4).map(Arc::new).collect();
        let mut held = EngineHeld::<i32, 2>::default();
        for arc in &arcs {
            held.hand(Arc::clone(arc));
        }

        assert_eq!(held.current().map(|arc| **arc), Some(3));
        let counts: Vec<_> = arcs.iter().map(Arc::strong_count).collect();
        assert_eq!(counts, [1, 2, 2, 2]);
    }

    #[test]
    fn a_retired_arc_stays_held_without_being_current() {
        let arc = Arc::new(7);
        let mut held = EngineHeld::<i32, 1>::default();
        held.hand(Arc::clone(&arc));

        held.retire();

        assert!(held.current().is_none());
        assert_eq!(Arc::strong_count(&arc), 2);
    }
}
