use vibez_core::id::EffectId;
use vibez_dsp::effect::AudioEffect;

pub struct EffectSlot {
    pub id: EffectId,
    pub effect: Box<dyn AudioEffect>,
    pub bypass: bool,
}
