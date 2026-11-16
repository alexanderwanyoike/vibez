//! Reads the MIDI files selected in the Browser for preview and hands their auditions to the engine.

use vibez_engine::commands::EngineCommand;

use super::App;

impl App {
    /// Tells the engine the hardware rate.
    pub(super) fn apply_engine_sample_rate(&mut self, sample_rate: u32) {
        self.send_command(EngineCommand::SetSampleRate(sample_rate));
    }
}
