use vibez_core::effect::{EffectType, ParamDescriptor};

use crate::effect::AudioEffect;

static REVERB_PARAMS: &[ParamDescriptor] = &[
    ParamDescriptor {
        name: "Room Size",
        min: 0.0,
        max: 1.0,
        default: 0.5,
        unit: "",
    },
    ParamDescriptor {
        name: "Damping",
        min: 0.0,
        max: 1.0,
        default: 0.5,
        unit: "",
    },
    ParamDescriptor {
        name: "Mix",
        min: 0.0,
        max: 1.0,
        default: 0.3,
        unit: "",
    },
];

// Freeverb-style comb filter delay lengths (in samples at 44100 Hz)
const COMB_LENGTHS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_LENGTHS: [usize; 4] = [556, 441, 341, 225];

struct CombFilter {
    buffer: Vec<f32>,
    pos: usize,
    feedback: f32,
    damp1: f32,
    damp2: f32,
    filter_store: f32,
}

impl CombFilter {
    fn new(size: usize) -> Self {
        Self {
            buffer: vec![0.0; size],
            pos: 0,
            feedback: 0.5,
            damp1: 0.5,
            damp2: 0.5,
            filter_store: 0.0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let output = self.buffer[self.pos];
        self.filter_store = output * self.damp2 + self.filter_store * self.damp1;
        self.buffer[self.pos] = input + self.filter_store * self.feedback;
        self.pos = (self.pos + 1) % self.buffer.len();
        output
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.filter_store = 0.0;
        self.pos = 0;
    }
}

struct AllPassFilter {
    buffer: Vec<f32>,
    pos: usize,
}

impl AllPassFilter {
    fn new(size: usize) -> Self {
        Self {
            buffer: vec![0.0; size],
            pos: 0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let buffered = self.buffer[self.pos];
        let output = -input + buffered;
        self.buffer[self.pos] = input + buffered * 0.5;
        self.pos = (self.pos + 1) % self.buffer.len();
        output
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.pos = 0;
    }
}

/// Freeverb-inspired stereo reverb.
pub struct ReverbEffect {
    room_size: f32,
    damping: f32,
    mix: f32,
    combs_l: Vec<CombFilter>,
    combs_r: Vec<CombFilter>,
    allpasses_l: Vec<AllPassFilter>,
    allpasses_r: Vec<AllPassFilter>,
}

impl ReverbEffect {
    pub fn new(sample_rate: f32) -> Self {
        let scale = sample_rate / 44100.0;
        let combs_l: Vec<CombFilter> = COMB_LENGTHS
            .iter()
            .map(|&len| CombFilter::new(((len as f32) * scale) as usize))
            .collect();
        let combs_r: Vec<CombFilter> = COMB_LENGTHS
            .iter()
            .map(|&len| CombFilter::new(((len as f32) * scale + 23.0) as usize))
            .collect();
        let allpasses_l: Vec<AllPassFilter> = ALLPASS_LENGTHS
            .iter()
            .map(|&len| AllPassFilter::new(((len as f32) * scale) as usize))
            .collect();
        let allpasses_r: Vec<AllPassFilter> = ALLPASS_LENGTHS
            .iter()
            .map(|&len| AllPassFilter::new(((len as f32) * scale + 23.0) as usize))
            .collect();

        let mut reverb = Self {
            room_size: 0.5,
            damping: 0.5,
            mix: 0.3,
            combs_l,
            combs_r,
            allpasses_l,
            allpasses_r,
        };
        reverb.update_combs();
        reverb
    }

    fn update_combs(&mut self) {
        let feedback = self.room_size * 0.28 + 0.7;
        let damp1 = self.damping * 0.4;
        let damp2 = 1.0 - damp1;

        for comb in self.combs_l.iter_mut().chain(self.combs_r.iter_mut()) {
            comb.feedback = feedback;
            comb.damp1 = damp1;
            comb.damp2 = damp2;
        }
    }
}

impl AudioEffect for ReverbEffect {
    fn effect_type(&self) -> EffectType {
        EffectType::Reverb
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        REVERB_PARAMS
    }

    fn set_param(&mut self, index: usize, value: f32) -> bool {
        match index {
            0 => {
                self.room_size = value.clamp(0.0, 1.0);
                self.update_combs();
                true
            }
            1 => {
                self.damping = value.clamp(0.0, 1.0);
                self.update_combs();
                true
            }
            2 => {
                self.mix = value.clamp(0.0, 1.0);
                true
            }
            _ => false,
        }
    }

    fn get_param(&self, index: usize) -> f32 {
        match index {
            0 => self.room_size,
            1 => self.damping,
            2 => self.mix,
            _ => 0.0,
        }
    }

    fn process(&mut self, buffer: &mut [f32], channels: usize) {
        let ch = channels.clamp(1, 2);
        let frames = buffer.len() / ch;

        for frame in 0..frames {
            let dry_l = buffer[frame * ch];
            let dry_r = if ch >= 2 {
                buffer[frame * ch + 1]
            } else {
                dry_l
            };

            let input = (dry_l + dry_r) * 0.5;

            let mut wet_l = 0.0_f32;
            let mut wet_r = 0.0_f32;

            for comb in &mut self.combs_l {
                wet_l += comb.process(input);
            }
            for comb in &mut self.combs_r {
                wet_r += comb.process(input);
            }

            for ap in &mut self.allpasses_l {
                wet_l = ap.process(wet_l);
            }
            for ap in &mut self.allpasses_r {
                wet_r = ap.process(wet_r);
            }

            buffer[frame * ch] = dry_l * (1.0 - self.mix) + wet_l * self.mix;
            if ch >= 2 {
                buffer[frame * ch + 1] = dry_r * (1.0 - self.mix) + wet_r * self.mix;
            }
        }
    }

    fn reset(&mut self) {
        for comb in self.combs_l.iter_mut().chain(self.combs_r.iter_mut()) {
            comb.reset();
        }
        for ap in self
            .allpasses_l
            .iter_mut()
            .chain(self.allpasses_r.iter_mut())
        {
            ap.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverb_silence_in_silence_out() {
        let mut fx = ReverbEffect::new(44100.0);
        let mut buf = vec![0.0_f32; 200];
        fx.process(&mut buf, 2);
        for s in &buf {
            assert!(s.abs() < 1e-6);
        }
    }

    #[test]
    fn reverb_produces_tail() {
        let mut fx = ReverbEffect::new(44100.0);
        fx.set_param(2, 1.0); // wet only

        // Impulse
        let mut buf = vec![0.0_f32; 4];
        buf[0] = 1.0;
        buf[1] = 1.0;
        fx.process(&mut buf, 2);

        // Now process more silence - should have reverb tail
        let mut tail = vec![0.0_f32; 4000];
        fx.process(&mut tail, 2);

        let energy: f32 = tail.iter().map(|s| s * s).sum();
        assert!(energy > 0.01, "Reverb should produce a tail");
    }

    #[test]
    fn reverb_dry_when_mix_zero() {
        let mut fx = ReverbEffect::new(44100.0);
        fx.set_param(2, 0.0);
        let mut buf = vec![0.5, -0.3, 0.8, -0.1];
        let orig = buf.clone();
        fx.process(&mut buf, 2);
        for (a, b) in buf.iter().zip(orig.iter()) {
            assert!((a - b).abs() < 1e-5);
        }
    }

    #[test]
    fn reverb_reset_clears() {
        let mut fx = ReverbEffect::new(44100.0);
        let mut buf = vec![1.0; 100];
        fx.process(&mut buf, 2);
        fx.reset();

        let mut silence = vec![0.0_f32; 200];
        fx.process(&mut silence, 2);
        for s in &silence {
            assert!(s.abs() < 1e-6);
        }
    }

    #[test]
    fn reverb_param_access() {
        let mut fx = ReverbEffect::new(44100.0);
        assert!((fx.get_param(0) - 0.5).abs() < 1e-3);
        assert!((fx.get_param(1) - 0.5).abs() < 1e-3);
        assert!((fx.get_param(2) - 0.3).abs() < 1e-3);
        fx.set_param(0, 0.8);
        assert!((fx.get_param(0) - 0.8).abs() < 1e-3);
        assert!(!fx.set_param(3, 1.0));
    }
}
