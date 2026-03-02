use vibez_core::effect::ParamDescriptor;

/// Trait for a loaded plugin instance ready for audio processing.
///
/// Implemented by both CLAP and VST3 host wrappers.
pub trait PluginInstance: Send {
    fn name(&self) -> &str;
    fn param_count(&self) -> usize;
    fn param_descriptors_vec(&self) -> Vec<ParamDescriptor>;
    fn set_param(&mut self, index: usize, value: f32) -> bool;
    fn get_param(&self, index: usize) -> f32;
    fn process_audio(&mut self, buffer: &mut [f32], channels: usize);
    fn note_on(&mut self, pitch: u8, velocity: u8);
    fn note_off(&mut self, pitch: u8);
    fn reset(&mut self);
    fn is_instrument(&self) -> bool;
    fn prepare(&mut self, sample_rate: f64, max_buffer_size: u32);
    fn activate(&mut self) -> bool;
    fn deactivate(&mut self);
}
