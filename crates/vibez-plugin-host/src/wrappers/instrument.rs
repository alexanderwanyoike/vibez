use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::effect::ParamDescriptor;
use vibez_core::midi::InstrumentKind;
use vibez_instruments::Instrument;

use crate::instance::PluginInstance;

/// Wraps a `Box<dyn PluginInstance>` to implement the `Instrument` trait,
/// allowing external plugins to slot into the existing instrument slot.
pub struct PluginInstrumentWrapper {
    inner: Box<dyn PluginInstance>,
    /// Leaked to satisfy the `&'static [ParamDescriptor]` requirement.
    descriptors: &'static [ParamDescriptor],
}

impl PluginInstrumentWrapper {
    pub fn new(inner: Box<dyn PluginInstance>) -> Self {
        let desc_vec = inner.param_descriptors_vec();
        let descriptors: &'static [ParamDescriptor] = Box::leak(desc_vec.into_boxed_slice());
        Self { inner, descriptors }
    }

    pub fn plugin_name(&self) -> &str {
        self.inner.name()
    }
}

impl Instrument for PluginInstrumentWrapper {
    fn instrument_kind(&self) -> InstrumentKind {
        // External plugins don't map to built-in InstrumentKind.
        // Use SubtractiveSynth as placeholder — UI uses plugin_name for display.
        InstrumentKind::SubtractiveSynth
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

    fn note_on(&mut self, pitch: u8, velocity: u8) {
        self.inner.note_on(pitch, velocity);
    }

    fn note_off(&mut self, pitch: u8) {
        self.inner.note_off(pitch);
    }

    fn note_on_at(&mut self, pitch: u8, velocity: u8, frame_offset: u32) {
        self.inner.note_on_at(pitch, velocity, frame_offset);
    }

    fn note_off_at(&mut self, pitch: u8, frame_offset: u32) {
        self.inner.note_off_at(pitch, frame_offset);
    }

    fn supports_batch_render(&self) -> bool {
        true
    }

    fn render(&mut self, buffer: &mut [f32], channels: usize) {
        // Catch panics from external plugin code to avoid crashing the audio thread.
        let inner = &mut self.inner;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            inner.process_audio(buffer, channels);
        }));
        if result.is_err() {
            buffer.fill(0.0);
            log::error!("Plugin panicked during render");
        }
    }

    fn reset(&mut self) {
        self.inner.reset();
    }

    fn load_sample(&mut self, _sample: Arc<DecodedAudio>, _name: String) {
        // Not applicable for external plugins
    }

    fn sample_name(&self) -> Option<&str> {
        None
    }
}
