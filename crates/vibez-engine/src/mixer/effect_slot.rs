//! Effect ownership, bypass and bounded per-insert meter observation.

use vibez_core::id::EffectId;
use vibez_dsp::effect::AudioEffect;

pub struct EffectSlot {
    pub id: EffectId,
    pub effect: Box<dyn AudioEffect>,
    pub bypass: bool,
    pub levels: crate::metering::EffectMeterState,
}
impl EffectSlot {
    pub(crate) fn observe_input(&mut self, samples: &[f32], channels: usize, generation: Option<u64>) {
        if let Some(generation) = generation.filter(|_| self.effect.supports_level_meter()) {
            self.levels.observe_input(samples, channels, generation);
        }
    }

    pub(crate) fn observe_output(&mut self, samples: &[f32], channels: usize, generation: Option<u64>) {
        if let Some(generation) = generation.filter(|_| self.effect.supports_level_meter()) {
            self.levels.observe_output(samples, channels, generation);
        }
    }

    pub(crate) fn process(&mut self, samples: &mut [f32], channels: usize, generation: Option<u64>) {
        self.observe_input(samples, channels, generation);
        if !self.bypass {
            self.effect.process(samples, channels);
        }
        self.observe_output(samples, channels, generation);
    }
}
