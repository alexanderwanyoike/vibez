use vibez_core::effect::{EffectType, ParamDescriptor};

pub trait AudioEffect: Send {
    fn effect_type(&self) -> EffectType;
    fn param_descriptors(&self) -> &'static [ParamDescriptor];
    fn set_param(&mut self, index: usize, value: f32) -> bool;
    fn get_param(&self, index: usize) -> f32;
    fn process(&mut self, buffer: &mut [f32], channels: usize);
    fn reset(&mut self);
    /// End an isolated offline processing run on its render thread.
    /// Native effects need no lifecycle transition.
    fn finish_offline_processing(&mut self) {}
}
