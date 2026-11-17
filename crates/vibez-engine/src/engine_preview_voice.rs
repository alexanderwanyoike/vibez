//! The instrument a MIDI audition plays on when no track can, mixed after the project's master.

use vibez_core::note_output::BlockNote;
use vibez_instruments::Instrument;

use crate::mixer::RoutedNotes;

pub(crate) struct PreviewVoice {
    instrument: Option<Box<dyn Instrument>>,
    notes: RoutedNotes<u64>,
    /// It made sound last block, so its release may still ring.
    sounding: bool,
}

impl PreviewVoice {
    pub(crate) fn new() -> Self {
        Self {
            instrument: None,
            notes: RoutedNotes::default(),
            sounding: false,
        }
    }

    /// The old instrument is returned for disposal off the audio thread.
    pub(crate) fn replace(
        &mut self,
        instrument: Box<dyn Instrument>,
    ) -> Option<Box<dyn Instrument>> {
        self.notes.release();
        self.instrument.replace(instrument)
    }

    /// Plays `notes` from the audition keyed `key` (none when it plays
    /// elsewhere) and mixes the result into `output` at `gain`. It renders
    /// only while it plays, still rings, or has a note-off to deliver.
    pub(crate) fn render(
        &mut self,
        key: Option<u64>,
        notes: &[BlockNote],
        output: &mut [f32],
        frames: usize,
        channels: usize,
        gain: f32,
    ) {
        self.notes.admit(key, notes, frames as u32);
        let Some(instrument) = self.instrument.as_mut() else {
            return;
        };
        if key.is_none() && !self.sounding && self.notes.notes().is_empty() {
            return;
        }
        let mut cursor = 0;
        let mut sounding = false;
        for frame in 0..frames {
            self.notes
                .play_due(frame as u32, &mut cursor, instrument.as_mut());
            let mut stereo = [0.0; 2];
            instrument.render(&mut stereo, 2);
            sounding |= stereo != [0.0; 2];
            for channel in 0..channels {
                output[frame * channels + channel] += stereo[channel.min(1)] * gain;
            }
        }
        self.sounding = sounding;
    }
}
