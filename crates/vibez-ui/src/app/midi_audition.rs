//! Reads the MIDI files selected in the Browser for preview and hands their auditions to the engine.

use std::sync::Arc;

use iced::Task;
use vibez_core::midi::InstrumentKind;
use vibez_core::track::MediaSourceRef;
use vibez_engine::commands::EngineCommand;

use super::midi_file_drop::read_midi_file_async;
use super::App;
use crate::domains::midi_audition::{audition_target, MidiPreview};
use crate::domains::midi_file_import::MidiFileImport;
use crate::message::Message;

impl App {
    /// Tells the engine the hardware rate, with a preview voice built for it.
    pub(super) fn apply_engine_sample_rate(&mut self, sample_rate: u32) {
        self.send_command(EngineCommand::SetSampleRate(sample_rate));
        self.send_command(EngineCommand::SetAuditionVoice(
            vibez_instruments::create_instrument(
                InstrumentKind::SubtractiveSynth,
                sample_rate as f32,
            ),
        ));
    }

    /// A click on a MIDI file reads it to show its pattern, auditioning it
    /// too while Audition is on; a click on the selected file plays it again.
    pub(super) fn select_midi_file(&mut self, source: MediaSourceRef) -> Task<Message> {
        if !self.state.browser.select_source(source.clone()) {
            return self.replay_selected_browser_sample();
        }
        self.state.status_text = format!(
            "Drag {} onto a MIDI track, empty Arrange or a MIDI track's Clip slot",
            source.display_name()
        );
        let play = self.state.browser.audition_enabled;
        self.read_midi_preview(source, play)
    }

    /// Play in the Audition footer, reading the file first if it is not yet.
    pub(super) fn preview_midi_file(&mut self, source: MediaSourceRef) -> Task<Message> {
        self.state.browser.select_source(source.clone());
        if self.audition_read_midi_file(&source) {
            return Task::none();
        }
        self.read_midi_preview(source, true)
    }

    /// Auditions `source` if it is a MIDI file already read; false if not.
    pub(super) fn audition_read_midi_file(&mut self, source: &MediaSourceRef) -> bool {
        let Some(preview) = self.state.browser.midi_preview_for(source).cloned() else {
            return false;
        };
        self.start_midi_audition(&preview);
        true
    }

    fn read_midi_preview(&mut self, source: MediaSourceRef, play: bool) -> Task<Message> {
        let MediaSourceRef::LocalFile { path } = source.clone() else {
            return Task::none();
        };
        let generation = if play {
            self.state.browser.begin_audition_load(&source)
        } else {
            self.state.browser.begin_waveform_load(&source);
            self.state.browser.audition_generation
        };
        Task::perform(read_midi_file_async(path), move |result| {
            Message::BrowserMidiPreviewReady {
                source: source.clone(),
                generation,
                play,
                result,
            }
        })
    }

    pub(super) fn on_midi_preview_ready(
        &mut self,
        source: MediaSourceRef,
        generation: u64,
        play: bool,
        result: Result<MidiFileImport, String>,
    ) {
        let play = play && self.state.browser.audition_request_is_current(generation);
        let installed = match result {
            Ok(import) => {
                let preview = MidiPreview::of(source, &import);
                self.state
                    .browser
                    .install_midi_preview(preview.clone())
                    .then_some(preview)
            }
            Err(error) => {
                self.state
                    .browser
                    .fail_waveform_load(&source, error.clone());
                if play {
                    self.stop_browser_audition();
                    self.state.status_text = format!("Audition error: {error}");
                }
                None
            }
        };
        match installed {
            Some(preview) if play => self.start_midi_audition(&preview),
            _ if play => self.state.browser.audition_loading = false,
            _ => {}
        }
    }

    /// Plays `preview` through the selected MIDI track's instrument, or the
    /// preview voice, in the chosen Raw or Warp treatment.
    pub(super) fn start_midi_audition(&mut self, preview: &MidiPreview) {
        let mode = self.state.browser.audition_mode;
        let target = audition_target(
            self.state.arrangement.selected_track,
            &self.state.project_tracks.tracks,
        );
        self.state.browser.audition_audio.retire();
        self.state
            .browser
            .audition_pattern
            .hand(Arc::clone(&preview.pattern));
        self.send_command(EngineCommand::StartMidiAudition {
            pattern: Arc::clone(&preview.pattern),
            target,
            start: mode.engine_start(),
            looped: mode.loops(),
        });
        self.audition_requested(mode);
    }
}
