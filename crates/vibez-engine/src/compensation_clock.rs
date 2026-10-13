//! Presentation history survives musical loop wraps without redefining the
//! engine's continuous processing clock.

use crate::compensation::MAX_PATH_LATENCY;
use crate::guarded_history::GuardedHistory;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PresentationPosition {
    pub arrange: u64,
    pub perform: u64,
    pub section: Option<u64>,
    pub section_id: Option<vibez_core::id::SectionId>,
    pub section_length: u64,
    pub generation: u64,
}

#[derive(Debug)]
pub struct PresentationHistory {
    positions: GuardedHistory<PresentationPosition>,
    delay: u32,
    performance_rebase_written: u64,
}

impl PresentationHistory {
    pub fn prepare(delay: u32) -> Result<Self, &'static str> {
        if delay > MAX_PATH_LATENCY {
            return Err("Presentation delay exceeds the supported path budget");
        }
        let count = GuardedHistory::<PresentationPosition>::required_count(delay, 0)?;
        let positions = GuardedHistory::prepare(count)?;
        Ok(Self {
            positions,
            delay,
            performance_rebase_written: 0,
        })
    }

    pub fn storage_bytes(&self) -> usize {
        self.positions.storage_bytes()
    }

    pub fn clear(&mut self) {
        self.positions.clear();
        self.performance_rebase_written = 0;
    }
    pub fn retain_from(&mut self, previous: &mut Self) -> bool {
        if self.delay != previous.delay || !self.positions.retain_from(&mut previous.positions) {
            return false;
        }
        self.performance_rebase_written = previous.performance_rebase_written;
        true
    }
    pub fn rebase_performance(&mut self) {
        // Old Arrange coordinates still describe audio draining through the
        // mix. Only their previous Perform lifetime becomes unavailable.
        self.performance_rebase_written = self.positions.written();
    }
    fn position(&self, index: u64) -> Option<PresentationPosition> {
        self.positions.get(index).map(|mut position| {
            if index < self.performance_rebase_written {
                position.perform = 0;
            }
            position
        })
    }

    /// Each span is a contiguous musical render segment. A loop or Section
    /// transition starts another span while retained output continues draining.
    pub fn record(&mut self, first: PresentationPosition, frames: usize) {
        self.record_clocks(first, frames, true, true);
    }

    pub fn record_clocks(
        &mut self,
        first: PresentationPosition,
        frames: usize,
        arrange_advances: bool,
        perform_advances: bool,
    ) {
        for offset in 0..frames {
            self.positions.push(PresentationPosition {
                arrange: first.arrange.saturating_add(if arrange_advances {
                    offset as u64
                } else {
                    0
                }),
                perform: first.perform.saturating_add(if perform_advances {
                    offset as u64
                } else {
                    0
                }),
                section: first
                    .section
                    .map(|local| local.saturating_add(offset as u64)),
                section_id: first.section_id,
                section_length: first.section_length,
                generation: first.generation,
            });
        }
    }

    pub fn before_block(&self, delay: u32) -> Option<PresentationPosition> {
        if delay == 0 || delay > self.delay {
            return None;
        }
        self.positions
            .written()
            .checked_sub(delay as u64)
            .and_then(|index| self.position(index))
    }
    pub fn audible(&self) -> Option<PresentationPosition> {
        self.positions
            .written()
            .checked_sub(self.delay as u64 + 1)
            .and_then(|index| self.position(index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presentation_follows_old_loop_position_until_its_audio_drains() {
        let mut history = PresentationHistory::prepare(3).unwrap();
        history.record(
            PresentationPosition {
                arrange: 100,
                perform: 0,
                section: Some(90),
                generation: 4,
                ..Default::default()
            },
            4,
        );
        assert_eq!(history.audible().unwrap().arrange, 100);
        history.record(
            PresentationPosition {
                arrange: 10,
                perform: 4,
                section: Some(0),
                generation: 4,
                ..Default::default()
            },
            2,
        );
        let old = history.audible().unwrap();
        assert_eq!((old.arrange, old.perform, old.section), (102, 2, Some(92)));
        history.record(
            PresentationPosition {
                arrange: 12,
                perform: 6,
                section: Some(2),
                generation: 5,
                ..Default::default()
            },
            2,
        );
        let wrapped = history.audible().unwrap();
        assert_eq!(
            (
                wrapped.arrange,
                wrapped.perform,
                wrapped.section,
                wrapped.generation
            ),
            (10, 4, Some(0), 4)
        );
    }

    #[test]
    fn preparation_and_seek_have_no_stale_audible_position() {
        let mut history = PresentationHistory::prepare(137).unwrap();
        history.record(PresentationPosition::default(), 137);
        assert_eq!(history.audible(), None);
        history.record(
            PresentationPosition {
                arrange: 137,
                ..Default::default()
            },
            1,
        );
        assert_eq!(history.audible().unwrap().arrange, 0);
        history.clear();
        history.record(
            PresentationPosition {
                arrange: 2000,
                ..Default::default()
            },
            137,
        );
        assert_eq!(history.audible(), None);
        history.record(
            PresentationPosition {
                arrange: 2137,
                ..Default::default()
            },
            1,
        );
        assert_eq!(history.audible().unwrap().arrange, 2000);
    }
}
