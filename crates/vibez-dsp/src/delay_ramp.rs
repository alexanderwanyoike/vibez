//! Owns sample-rate manual Delay scalar ramps and exact automation/reset targets.

pub(super) struct ScalarRamp {
    current: f32,
    origin: f32,
    target: f32,
    frame: usize,
    frames: usize,
    started: bool,
}

impl ScalarRamp {
    pub(super) fn new(value: f32) -> Self {
        Self {
            current: value,
            origin: value,
            target: value,
            frame: 0,
            frames: 0,
            started: false,
        }
    }

    fn snap(&mut self, target: f32) {
        self.current = target;
        self.origin = target;
        self.target = target;
        self.frame = 0;
        self.frames = 0;
    }

    pub(super) fn edit(&mut self, target: f32, automated: bool, frames: usize) {
        if !self.started || automated {
            self.snap(target);
            return;
        }
        self.origin = self.current;
        self.target = target;
        self.frame = 0;
        self.frames = frames;
    }

    pub(super) fn next_frame(&mut self) -> f32 {
        self.started = true;
        if self.frame < self.frames {
            self.frame += 1;
            self.current = if self.frame == self.frames {
                self.target
            } else {
                // Origin-based progress avoids drift and retargets from the last audible coefficient.
                (f64::from(self.origin)
                    + (f64::from(self.target) - f64::from(self.origin)) * self.frame as f64
                        / self.frames as f64) as f32
            };
        }
        self.current
    }

    pub(super) fn reset(&mut self, target: f32) {
        self.snap(target);
        self.started = false;
    }
}
