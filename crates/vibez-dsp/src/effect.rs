use vibez_core::effect::{EffectType, ParamDescriptor};

use vibez_core::routing::{ExternalInputBlock, ExternalInputDescriptor};

pub trait AudioEffect: Send {
    fn external_inputs(&self) -> &[ExternalInputDescriptor] {
        &[]
    }
    fn process_with_inputs(
        &mut self,
        buffer: &mut [f32],
        channels: usize,
        _inputs: &[ExternalInputBlock<'_>],
    ) {
        self.process(buffer, channels);
    }
    fn set_audio_context(&mut self, _context: vibez_core::audio_context::DeviceAudioContext) {}
    fn reconfiguration_requested(&self) -> bool {
        false
    }
    /// Called on the processing thread before transferring exclusive ownership.
    fn stop_for_reconfiguration(&mut self) {}
    /// Called on the format's main thread, after processing has stopped.
    fn reconfigure_on_main_thread(&mut self) -> Result<(), String> {
        Ok(())
    }
    /// None permits rate-independent processors without forcing recreation.
    fn activation_sample_rate(&self) -> Option<u32> {
        None
    }
    fn processing_configuration_valid(&self) -> bool {
        true
    }
    /// Cached processing delay, excluding musical echoes and hardware delay.
    /// Format adapters refresh this only during their permitted lifecycle.
    fn latency_samples(&self) -> u32 {
        0
    }
    fn effect_type(&self) -> EffectType;
    fn param_descriptors(&self) -> &'static [ParamDescriptor];
    fn set_param(&mut self, index: usize, value: f32) -> bool;
    fn get_param(&self, index: usize) -> f32;
    fn process(&mut self, buffer: &mut [f32], channels: usize);
    fn reset(&mut self);
    /// End an isolated offline processing run on its render thread.
    /// Native effects need no lifecycle transition.
    fn stop_processing(&mut self) {}
    fn finish_offline_processing(&mut self) {
        self.stop_processing();
    }
}
