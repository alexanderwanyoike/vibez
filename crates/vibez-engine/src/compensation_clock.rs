//! Presentation history survives musical loop wraps without redefining the
//! engine's continuous processing clock.

use crate::compensation::MAX_PATH_LATENCY;

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
    positions: Vec<PresentationPosition>,
    written: u64,
    delay: u32,
}

impl PresentationHistory {
    pub fn prepare(delay: u32) -> Result<Self, &'static str> {
        if delay > MAX_PATH_LATENCY {
            return Err("Presentation delay exceeds the supported path budget");
        }
        let mut positions = Vec::new();
        positions
            .try_reserve_exact(delay as usize + 1)
            .map_err(|_| "Unable to allocate presentation history")?;
        positions.resize(delay as usize + 1, PresentationPosition::default());
        Ok(Self {
            positions,
            written: 0,
            delay,
        })
    }

    pub fn clear(&mut self) {
        self.written = 0;
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
            let index = (self.written % self.positions.len() as u64) as usize;
            self.positions[index] = PresentationPosition {
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
            };
            self.written += 1;
        }
    }

    pub fn before_block(&self, delay: u32) -> Option<PresentationPosition> {
        if delay == 0 || delay > self.delay || self.written < delay as u64 {
            return None;
        }
        let index = self.written - delay as u64;
        Some(self.positions[(index % self.positions.len() as u64) as usize])
    }
    pub fn audible(&self) -> Option<PresentationPosition> {
        let index = self.written.checked_sub(self.delay as u64 + 1)?;
        Some(self.positions[(index % self.positions.len() as u64) as usize])
    }
}

/// A live input sounds after its actual remaining path delay. Map that onset
/// to the compensated mix heard at the same instant, including a reduced
/// monitoring branch's accepted earlier presentation.
pub fn capture_live_position(
    render_position: u64,
    mix_latency: u32,
    live_path_latency: u32,
) -> u64 {
    render_position.saturating_sub(mix_latency.saturating_sub(live_path_latency) as u64)
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

    #[test]
    fn capture_maps_full_and_reduced_branches_against_compensated_mix() {
        assert_eq!(capture_live_position(4000, 521, 521), 4000);
        assert_eq!(capture_live_position(4000, 521, 137), 3616);
        assert_eq!(capture_live_position(4000, 521, 0), 3479);
    }
}
