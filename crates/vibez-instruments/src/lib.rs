//! Instrument rendering and native ownership configuration.

pub mod drum_rack;
pub(crate) mod envelope;
pub mod fm;
pub mod fm_params;
mod fm_voice;
mod media_owners;
mod poly;
mod prepared_pad_state;
pub use media_owners::InstrumentMediaOwners;
pub use prepared_pad_state::PreparedDrumPadState;
pub mod sampler;
pub mod sampler_params;
mod sampler_voice;
pub mod synth;
mod synth_filter;
pub mod synth_oscillator;
pub mod synth_params;
mod synth_voice;
pub mod wavetable;
pub mod wavetable_params;
mod wavetable_voice;
pub mod wavetables;

use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::effect::ParamDescriptor;
use vibez_core::midi::InstrumentKind;

use drum_rack::DrumRack;
use fm::FmSynth;
use sampler::Sampler;
use synth::SubtractiveSynth;
use wavetable::WavetableSynth;

/// Trait implemented by all instruments (synths, samplers, etc.).
/// Parameter descriptors for an instrument kind, without building
/// an instance (UI pickers, automation lane labels).
pub fn descriptors_for(kind: InstrumentKind) -> &'static [ParamDescriptor] {
    match kind {
        InstrumentKind::SubtractiveSynth => synth::SYNTH_PARAMS,
        InstrumentKind::WavetableSynth => wavetable::WAVETABLE_PARAMS,
        InstrumentKind::FmSynth => fm::FM_PARAMS,
        InstrumentKind::Sampler => sampler::SAMPLER_PARAMS,
        // The drum rack has per-pad state, no top-level params.
        InstrumentKind::DrumRack => &[],
    }
}

pub trait Instrument: Send {
    fn set_audio_context(&mut self, _context: vibez_core::audio_context::DeviceAudioContext) {}
    /// A process/start failure can silence this device without invalidating the
    /// prepared latency graph. Only main-thread reactivation clears its gate.
    fn processing_failure_is_local(&self) -> bool {
        false
    }

    /// Requests bounded main-thread recovery, never a callback-side DSP retry.
    fn processing_recovery_requested(&self) -> bool {
        false
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
    fn instrument_kind(&self) -> InstrumentKind;
    fn param_descriptors(&self) -> &'static [ParamDescriptor];
    fn set_param(&mut self, index: usize, value: f32) -> bool;
    fn get_param(&self, index: usize) -> f32;
    fn note_on(&mut self, pitch: u8, velocity: u8);
    fn note_off(&mut self, pitch: u8);
    /// Schedule a note-on at a specific frame offset (for batch rendering).
    fn note_on_at(&mut self, pitch: u8, velocity: u8, _frame_offset: u32) {
        self.note_on(pitch, velocity);
    }
    /// Schedule a note-off at a specific frame offset (for batch rendering).
    fn note_off_at(&mut self, pitch: u8, _frame_offset: u32) {
        self.note_off(pitch);
    }
    fn render(&mut self, buffer: &mut [f32], channels: usize);
    fn reset(&mut self);
    /// End an isolated offline processing run on its render thread.
    /// Native instruments need no lifecycle transition.
    /// A cached diagnostic consumed without formatting or I/O in processing.
    fn take_processing_error(&mut self) -> Option<&'static str> {
        None
    }
    fn stop_processing(&mut self) {}
    fn finish_offline_processing(&mut self) {
        self.stop_processing();
    }
    /// Whether this instrument supports batch rendering with timed events.
    /// When true, the mixer will call note_on_at/note_off_at with frame
    /// offsets and then render() once for the entire buffer.
    fn supports_batch_render(&self) -> bool {
        false
    }

    /// Load a sample into the instrument. No-op for non-sample instruments.
    #[must_use = "return displaced or rejected media to its owner thread"]
    fn load_sample(&mut self, sample: Arc<DecodedAudio>, name: String) -> InstrumentMediaOwners {
        InstrumentMediaOwners::incoming(sample, name)
    }

    /// Load a sample into a drum-rack pad. No-op for non-drum-rack instruments.
    #[must_use = "return displaced or rejected media to its owner thread"]
    fn load_drum_pad_sample(
        &mut self,
        _pad_index: usize,
        sample: Arc<DecodedAudio>,
        name: String,
    ) -> InstrumentMediaOwners {
        InstrumentMediaOwners::incoming(sample, name)
    }

    /// Clear a drum-rack pad. No-op for non-drum-rack instruments.
    #[must_use = "return displaced media to its owner thread"]
    fn clear_drum_pad(&mut self, _pad_index: usize) -> InstrumentMediaOwners {
        InstrumentMediaOwners::default()
    }

    /// Apply pad settings to a drum-rack pad. No-op for other instruments.
    #[must_use = "return displaced or rejected names to their owner thread"]
    fn set_drum_pad_state(
        &mut self,
        _pad_index: usize,
        state: PreparedDrumPadState,
    ) -> InstrumentMediaOwners {
        InstrumentMediaOwners {
            sample: None,
            name: state.name,
        }
    }

    /// Set one of a drum-rack pad's Sampler controls. No-op for other
    /// instruments.
    fn set_drum_pad_param(&mut self, _pad_index: usize, _index: usize, _value: f32) {}

    /// Sample name for UI display. None for non-sample instruments.
    fn sample_name(&self) -> Option<&str> {
        None
    }
}

/// Factory function to create an instrument by kind.
pub fn create_instrument(kind: InstrumentKind, sample_rate: f32) -> Box<dyn Instrument> {
    match kind {
        InstrumentKind::SubtractiveSynth => Box::new(SubtractiveSynth::new(sample_rate)),
        InstrumentKind::WavetableSynth => Box::new(WavetableSynth::new(sample_rate)),
        InstrumentKind::FmSynth => Box::new(FmSynth::new(sample_rate)),
        InstrumentKind::Sampler => Box::new(Sampler::new(sample_rate)),
        InstrumentKind::DrumRack => Box::new(DrumRack::new(sample_rate)),
    }
}
