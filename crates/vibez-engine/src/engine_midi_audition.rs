//! The Browser's MIDI audition: when its notes start, which instrument plays them, and the preview voice that plays them when no track can.

use std::sync::Arc;

use rtrb::Producer;
use vibez_core::id::TrackId;
use vibez_core::note_output::{BlockNote, NoteOutput};
use vibez_instruments::Instrument;

use crate::engine_audition::frames_until_audition_boundary;
use crate::events::EngineEvent;
use crate::midi_audition::MidiAuditionPattern;
use crate::mixer::{EngineTrack, RoutedNotes};

/// Who plays this block's audition notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Player {
    Track(TrackId),
    PreviewVoice,
}

pub(crate) struct MidiAuditionVoice {
    pattern: Arc<MidiAuditionPattern>,
    target: Option<TrackId>,
    /// Samples played since the audition started.
    position: u64,
    looped: bool,
    synchronized: bool,
}

impl MidiAuditionVoice {
    pub(crate) fn new(
        pattern: Arc<MidiAuditionPattern>,
        target: Option<TrackId>,
        looped: bool,
        synchronized: bool,
    ) -> Self {
        Self {
            pattern,
            target,
            position: 0,
            looped,
            synchronized,
        }
    }
}

struct QueuedMidiAudition {
    voice: MidiAuditionVoice,
    frames_until_start: u64,
}

pub(crate) struct MidiAudition {
    active: Option<MidiAuditionVoice>,
    queued: Option<QueuedMidiAudition>,
    /// Bumped by every start, so the instrument playing an audition releases
    /// what the one before it held.
    generation: u64,
    /// The notes the audition plays this block, and who plays them.
    block: NoteOutput,
    /// The block's notes before a start inside the block delays them.
    pattern_notes: NoteOutput,
    player: Option<Player>,
    preview_voice: Option<Box<dyn Instrument>>,
    preview_notes: RoutedNotes<u64>,
}

impl MidiAudition {
    pub(crate) fn new() -> Self {
        Self {
            active: None,
            queued: None,
            generation: 0,
            block: NoteOutput::default(),
            pattern_notes: NoteOutput::default(),
            player: None,
            preview_voice: None,
            preview_notes: RoutedNotes::default(),
        }
    }

    pub(crate) fn start(&mut self, voice: MidiAuditionVoice) {
        self.queued = None;
        self.generation = self.generation.wrapping_add(1);
        self.active = Some(voice);
    }

    pub(crate) fn queue(&mut self, voice: MidiAuditionVoice, frames_until_start: u64) {
        self.active = None;
        self.queued = Some(QueuedMidiAudition {
            voice,
            frames_until_start,
        });
    }

    /// Ends the audition; whoever holds its notes releases them at the next
    /// block start.
    pub(crate) fn stop(&mut self) {
        self.active = None;
        self.queued = None;
    }

    pub(crate) fn is_running(&self) -> bool {
        self.active.is_some() || self.queued.is_some()
    }

    /// The old voice is returned for disposal off the audio thread.
    pub(crate) fn replace_preview_voice(
        &mut self,
        voice: Box<dyn Instrument>,
    ) -> Option<Box<dyn Instrument>> {
        self.preview_notes.release();
        self.preview_voice.replace(voice)
    }

    /// Restarts a synchronized audition on the bar the transport starts on,
    /// or queues it for the next one; `Some(true)` when queued.
    pub(crate) fn resync_on_transport_start(
        &mut self,
        position: u64,
        bpm: f64,
        sample_rate: u32,
    ) -> Option<bool> {
        let active = self.active.take_if(|voice| voice.synchronized)?;
        let voice = MidiAuditionVoice::new(active.pattern, active.target, active.looped, true);
        let frames_until_start = frames_until_audition_boundary(position, bpm, sample_rate, 4);
        if frames_until_start == 0 {
            self.start(voice);
            Some(false)
        } else {
            self.queue(voice, frames_until_start);
            Some(true)
        }
    }

    /// Works out this block's notes and who plays them, before any track
    /// renders. `playable` says whether a track has an instrument to play
    /// them on.
    pub(crate) fn sequence(
        &mut self,
        frames: usize,
        playing: bool,
        samples_per_beat: f64,
        playable: impl Fn(TrackId) -> bool,
        event_tx: &mut Producer<EngineEvent>,
    ) {
        self.block.clear();
        let mut lead = 0;
        let starts = self
            .queued
            .as_ref()
            .is_some_and(|queued| !playing || queued.frames_until_start < frames as u64);
        if starts {
            let queued = self.queued.take().expect("queued audition exists");
            if playing {
                lead = queued.frames_until_start as usize;
            }
            self.start(queued.voice);
            let _ = event_tx.push(EngineEvent::AuditionStarted);
        } else if playing {
            if let Some(queued) = self.queued.as_mut() {
                queued.frames_until_start -= frames as u64;
            }
        }
        let Some(voice) = self.active.as_mut() else {
            self.player = None;
            return;
        };
        let heard = frames - lead;
        voice.pattern.collect(
            voice.position,
            heard,
            samples_per_beat,
            voice.looped,
            &mut self.pattern_notes,
        );
        for &note in self.pattern_notes.notes() {
            self.block.push(BlockNote {
                frame: note.frame + lead as u32,
                ..note
            });
        }
        voice.position += heard as u64;
        self.player = Some(match voice.target {
            Some(track) if playable(track) => Player::Track(track),
            _ => Player::PreviewVoice,
        });
        let _ = event_tx.push(EngineEvent::AuditionPosition(voice.position));
        if voice
            .pattern
            .ended(voice.position, samples_per_beat, voice.looped)
        {
            self.active = None;
            let _ = event_tx.push(EngineEvent::AuditionStopped);
        }
    }

    /// The key and notes `player` admits this block, which are none unless
    /// it plays them.
    fn notes_for(&self, player: Player) -> (Option<u64>, &[BlockNote]) {
        notes_for(self.player, self.generation, &self.block, player)
    }

    /// Hands `track` this block's notes when it plays them, and otherwise
    /// lets it release any it still holds.
    pub(crate) fn admit_into(&self, track: &mut EngineTrack, frames: u32) {
        let (key, notes) = self.notes_for(Player::Track(track.id));
        track.audition_notes.admit(key, notes, frames);
    }

    /// Mixes the preview voice into `output`, after the project's master.
    pub(crate) fn render_preview_voice(
        &mut self,
        output: &mut [f32],
        frames: usize,
        channels: usize,
        gain: f32,
    ) {
        let (key, notes) = notes_for(
            self.player,
            self.generation,
            &self.block,
            Player::PreviewVoice,
        );
        self.preview_notes.admit(key, notes, frames as u32);
        let Some(voice) = self.preview_voice.as_mut() else {
            return;
        };
        let mut cursor = 0;
        for frame in 0..frames {
            self.preview_notes
                .play_due(frame as u32, &mut cursor, voice.as_mut());
            let mut stereo = [0.0; 2];
            voice.render(&mut stereo, 2);
            for channel in 0..channels {
                output[frame * channels + channel] += stereo[channel.min(1)] * gain;
            }
        }
    }
}

fn notes_for(
    playing: Option<Player>,
    generation: u64,
    block: &NoteOutput,
    player: Player,
) -> (Option<u64>, &[BlockNote]) {
    if playing == Some(player) {
        (Some(generation), block.notes())
    } else {
        (None, &[])
    }
}
