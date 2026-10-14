//! A latched interval shares acquisition phase across the processed stereo pair.

use super::random::Random;

#[derive(Clone, Copy)]
pub(super) struct Clock {
    counter: u32,
    divider: u32,
    offset: f64,
    interval: u32,
}

impl Default for Clock {
    fn default() -> Self {
        Self {
            counter: 0,
            divider: 1,
            offset: 0.0,
            interval: 1,
        }
    }
}

impl Clock {
    pub(super) fn advance(
        &mut self,
        divider: u32,
        jitter: f32,
        frame: u64,
        random: &Random,
    ) -> bool {
        if divider != self.divider {
            self.divider = divider;
            self.update_interval();
        }
        let acquired = self.counter == 0;
        if acquired {
            self.offset = 0.5 * f64::from(jitter) * random.clock(frame);
            self.update_interval();
        }
        // Reducing an interval preserves the old counter until the original modulo step.
        self.counter = (self.counter + 1) % self.interval;
        acquired
    }

    pub(super) fn interval(&self) -> u32 {
        self.interval
    }

    fn update_interval(&mut self) {
        self.interval = (f64::from(self.divider) * (1.0 + self.offset)).round().max(1.0) as u32;
    }
}
