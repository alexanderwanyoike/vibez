use vibez_core::effect::{EffectType, ParamDescriptor};
use vibez_dsp::effect::AudioEffect;

use crate::instance::PluginInstance;

/// Wraps a `Box<dyn PluginInstance>` to implement the `AudioEffect` trait,
/// allowing external plugins to slot into the existing effect chain.
pub struct PluginEffectWrapper {
    inner: Box<dyn PluginInstance>,
    processing_error: Option<&'static str>,
    /// Leaked to satisfy the `&'static [ParamDescriptor]` requirement.
    descriptors: &'static [ParamDescriptor],
}

impl PluginEffectWrapper {
    pub fn new(inner: Box<dyn PluginInstance>) -> Self {
        let desc_vec = inner.param_descriptors_vec();
        let descriptors: &'static [ParamDescriptor] = Box::leak(desc_vec.into_boxed_slice());
        Self {
            inner,
            descriptors,
            processing_error: None,
        }
    }

    pub fn plugin_name(&self) -> &str {
        self.inner.name()
    }
}

impl AudioEffect for PluginEffectWrapper {
    fn external_inputs(&self) -> &[vibez_core::routing::ExternalInputDescriptor] {
        self.inner.external_inputs()
    }
    fn process_with_inputs(
        &mut self,
        buffer: &mut [f32],
        channels: usize,
        inputs: &[vibez_core::routing::ExternalInputBlock<'_>],
    ) {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.inner.process_with_inputs(buffer, channels, inputs)
        }));
        if result.is_err() {
            buffer.fill(0.0);
        }
    }

    fn effect_type(&self) -> EffectType {
        // External plugins don't map to built-in EffectType.
        // We use Gain as a placeholder — the UI uses plugin_name for display.
        EffectType::Gain
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        self.descriptors
    }

    fn set_param(&mut self, index: usize, value: f32) -> bool {
        self.inner.set_param(index, value)
    }

    fn get_param(&self, index: usize) -> f32 {
        self.inner.get_param(index)
    }

    fn process(&mut self, buffer: &mut [f32], channels: usize) {
        // Catch panics from external plugin code to avoid crashing the audio thread.
        let inner = &mut self.inner;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            inner.process_audio(buffer, channels);
        }));
        if result.is_err() {
            // Plugin panicked — zero the buffer to avoid noise.
            buffer.fill(0.0);
            self.processing_error = Some("Plugin panicked during processing");
        }
    }

    fn reset(&mut self) {
        self.inner.reset();
    }

    fn take_processing_error(&mut self) -> Option<&'static str> {
        self.processing_error
            .take()
            .or_else(|| self.inner.take_processing_error())
    }

    fn stop_processing(&mut self) {
        self.inner.stop_processing();
    }
}
