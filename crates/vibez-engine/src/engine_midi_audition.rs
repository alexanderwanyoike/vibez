//! The Browser's MIDI audition, worked out block by block into the notes it plays and the instrument that plays them.

use std::sync::Arc;

use rtrb::Producer;
use vibez_core::id::TrackId;
use vibez_core::note_output::{BlockNote, NoteOutput};
use vibez_instruments::Instrument;

use crate::engine_audition::frames_until_audition_boundary;
use crate::engine_audition_queue::{take_due, QueuedStart};
use crate::engine_preview_voice::PreviewVoice;
use crate::events::EngineEvent;
use crate::midi_audition::MidiAuditionPattern;
use crate::mixer::EngineTrack;

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

pub(crate) struct MidiAudition {
    active: Option<MidiAuditionVoice>,
    queued: Option<QueuedStart<MidiAuditionVoice>>,
    /// Bumped by every start, so the instrument playing an audition releases
    /// what the one before it held.
    generation: u64,
    /// The notes the audition plays this block, and who plays them.
    block: NoteOutput,
    /// The block's notes before a start inside the block delays them.
    pattern_notes: NoteOutput,
    player: Option<Player>,
    preview_voice: PreviewVoice,
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
            preview_voice: PreviewVoice::new(),
        }
    }

    pub(crate) fn start(&mut self, voice: MidiAuditionVoice) {
        self.queued = None;
        self.generation = self.generation.wrapping_add(1);
        self.active = Some(voice);
    }

    pub(crate) fn queue(&mut self, voice: MidiAuditionVoice, frames_until_start: u64) {
        self.active = None;
        self.queued = Some(QueuedStart::new(voice, frames_until_start));
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
        if let Some((voice, starts_at)) = take_due(&mut self.queued, frames, playing) {
            lead = starts_at;
            self.start(voice);
            let _ = event_tx.push(EngineEvent::AuditionStarted);
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

    /// Hands `track` this block's notes when it plays them, and otherwise
    /// lets it release any it still holds.
    pub(crate) fn admit_into(&self, track: &mut EngineTrack, frames: u32) {
        let (key, notes) = notes_for(
            self.player,
            self.generation,
            &self.block,
            Player::Track(track.id),
        );
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
        self.preview_voice
            .render(key, notes, output, frames, channels, gain);
    }
}

/// The key and notes `player` admits this block, which are none unless it
/// plays them.
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
