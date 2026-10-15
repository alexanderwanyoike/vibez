//! Owns linked input detection and wet-only Delay ducking with independent amount edits.

use super::ramp::ScalarRamp;

pub(super) struct Duck {
    amount: ScalarRamp,
    envelope: f64,
    attack: f64,
    release: f64,
}

impl Default for Duck {
    fn default() -> Self {
        Self { amount: ScalarRamp::new(0.0), envelope: 0.0, attack: 0.0, release: 0.0 }
    }
}

impl Duck {
    pub(super) fn new(sample_rate: f32) -> Self {
        let rate = f64::from(sample_rate);
        Self { attack: (-1.0 / (0.005 * rate)).exp(), release: (-1.0 / (0.150 * rate)).exp(),
            ..Default::default() }
    }

    pub(super) fn edit(&mut self, target: f32, automated: bool, frames: usize) {
        self.amount.edit(target, automated, frames);
    }

    pub(super) fn process(&mut self, input: [f32; 2], wet: [f32; 2]) -> [f32; 2] {
        let peak = f64::from(input[0].abs().max(input[1].abs()));
        let coefficient = if peak > self.envelope { self.attack } else { self.release };
        self.envelope = coefficient * self.envelope + (1.0 - coefficient) * peak;
        let amount = self.amount.next_frame();
        // Unity bypass still observes input so later enabling cannot expose a stale envelope.
        if amount == 0.0 {
            return wet;
        }
        let attenuation = 24.0 * f64::from(amount) * (self.envelope / 0.25).clamp(0.0, 1.0);
        let gain = 10.0_f64.powf(-attenuation / 20.0);
        wet.map(|sample| (f64::from(sample) * gain) as f32)
    }

    pub(super) fn attenuation_db(&self) -> f32 {
        (24.0 * f64::from(self.amount.current()) * (self.envelope / 0.25).clamp(0.0, 1.0)) as f32
    }

    pub(super) fn reset(&mut self, target: f32) {
        self.envelope = 0.0;
        self.amount.reset(target);
    }
}
