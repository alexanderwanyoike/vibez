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
    fn reconfiguration_requested(&self) -> bool {
        false
    }
    /// Called on the processing thread before transferring exclusive ownership.
    fn stop_for_reconfiguration(&mut self) {}
    /// Called on the format's main thread, after processing has stopped.
    fn reconfigure_on_main_thread(&mut self) -> Result<(), String> {
        Ok(())
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
    /// A cached diagnostic consumed without formatting or I/O in processing.
    fn take_processing_error(&mut self) -> Option<&'static str> {
        None
    }
    fn stop_processing(&mut self) {}
    /// End an isolated offline processing run on its render thread.
    /// Native effects need no lifecycle transition.
    fn finish_offline_processing(&mut self) {
        self.stop_processing();
    }
}
