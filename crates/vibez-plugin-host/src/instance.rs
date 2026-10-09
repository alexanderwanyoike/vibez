use vibez_core::effect::ParamDescriptor;

/// Trait for a loaded plugin instance ready for audio processing.
///
/// Implemented by both CLAP and VST3 host wrappers.
pub trait PluginInstance: Send {
    fn external_inputs(&self) -> &[vibez_core::routing::ExternalInputDescriptor] {
        &[]
    }
    fn process_with_inputs(
        &mut self,
        buffer: &mut [f32],
        channels: usize,
        _inputs: &[vibez_core::routing::ExternalInputBlock<'_>],
    ) {
        self.process_audio(buffer, channels);
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
    fn name(&self) -> &str;
    fn param_count(&self) -> usize;
    fn param_descriptors_vec(&self) -> Vec<ParamDescriptor>;
    fn set_param(&mut self, index: usize, value: f32) -> bool;
    fn get_param(&self, index: usize) -> f32;
    fn process_audio(&mut self, buffer: &mut [f32], channels: usize);
    fn note_on(&mut self, pitch: u8, velocity: u8);
    fn note_off(&mut self, pitch: u8);
    /// Schedule a note-on at a specific frame offset within the next process buffer.
    fn note_on_at(&mut self, pitch: u8, velocity: u8, frame_offset: u32) {
        let _ = frame_offset;
        self.note_on(pitch, velocity);
    }
    /// Schedule a note-off at a specific frame offset within the next process buffer.
    fn note_off_at(&mut self, pitch: u8, frame_offset: u32) {
        let _ = frame_offset;
        self.note_off(pitch);
    }
    fn reset(&mut self);
    /// Capture the plugin's opaque state blob (project persistence).
    /// Main-thread class in both CLAP and VST3. Default: unsupported.
    fn save_state(&mut self) -> Option<Vec<u8>> {
        None
    }
    /// Restore a previously captured state blob. Default: unsupported.
    fn load_state(&mut self, _data: &[u8]) -> bool {
        false
    }
    fn is_instrument(&self) -> bool;
    fn prepare(&mut self, sample_rate: f64, max_buffer_size: u32);
    fn activate(&mut self) -> bool;
    /// Stop the realtime processing phase on the processing thread while
    /// keeping the main-thread-owned instance alive for later teardown.
    /// A cached diagnostic consumed without formatting or I/O in processing.
    fn take_processing_error(&mut self) -> Option<&'static str> {
        None
    }
    fn stop_processing(&mut self) {}
    fn deactivate(&mut self);
}
