pub(crate) mod envelope;
pub mod sampler;
pub mod synth;

use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::effect::ParamDescriptor;
use vibez_core::midi::InstrumentKind;

use sampler::Sampler;
use synth::SubtractiveSynth;

/// Trait implemented by all instruments (synths, samplers, etc.).
pub trait Instrument: Send {
    fn instrument_kind(&self) -> InstrumentKind;
    fn param_descriptors(&self) -> &'static [ParamDescriptor];
    fn set_param(&mut self, index: usize, value: f32) -> bool;
    fn get_param(&self, index: usize) -> f32;
    fn note_on(&mut self, pitch: u8, velocity: u8);
    fn note_off(&mut self, pitch: u8);
    fn render(&mut self, buffer: &mut [f32], channels: usize);
    fn reset(&mut self);

    /// Load a sample into the instrument. No-op for non-sample instruments.
    fn load_sample(&mut self, _sample: Arc<DecodedAudio>, _name: String) {}

    /// Sample name for UI display. None for non-sample instruments.
    fn sample_name(&self) -> Option<&str> {
        None
    }
}

/// Factory function to create an instrument by kind.
pub fn create_instrument(kind: InstrumentKind, sample_rate: f32) -> Box<dyn Instrument> {
    match kind {
        InstrumentKind::SubtractiveSynth => Box::new(SubtractiveSynth::new(sample_rate)),
        InstrumentKind::Sampler => Box::new(Sampler::new(sample_rate)),
    }
}
