//! Source-frame control ramps keep manual edits independent of callback partitioning.

const RAMP_FRAMES: u32 = 512;

pub(super) struct Ramp {
    value: f32,
    step: f32,
    remaining: u32,
}

impl Ramp {
    pub(super) fn new(value: f32) -> Self {
        Self {
            value,
            step: 0.0,
            remaining: 0,
        }
    }

    pub(super) fn value(&self) -> f32 {
        self.value
    }

    pub(super) fn edit(&mut self, target: f32, immediate: bool) {
        if immediate {
            *self = Self::new(target);
        } else {
            self.step = (target - self.value) / RAMP_FRAMES as f32;
            self.remaining = RAMP_FRAMES;
        }
    }

    pub(super) fn advance(&mut self, target: f32) -> Option<f32> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        self.value = if self.remaining == 0 {
            target
        } else {
            self.value + self.step
        };
        Some(self.value)
    }
}
