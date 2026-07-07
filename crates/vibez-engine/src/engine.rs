use std::sync::Arc;

use rtrb::{Consumer, Producer, RingBuffer};
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::constants::RING_BUFFER_CAPACITY;

use vibez_core::time::TempoMap;
use vibez_dsp::factory::create_effect;
use vibez_instruments::create_instrument;

use crate::commands::EngineCommand;
use crate::events::EngineEvent;
use crate::metering;
use crate::mixer::{
    any_solo, calculate_total_length, equal_power_pan, EffectSlot, EngineClip, EngineNoteClip,
    EngineTrack,
};
use crate::transport::Transport;

/// The real-time audio engine.
///
/// `AudioEngine` lives on the audio thread.  Its [`process()`](AudioEngine::process)
/// method is called once per audio callback to fill an output buffer with audio
/// and communicate with the UI thread via lock-free ring buffers.
///
/// # Construction
///
/// Use [`AudioEngine::new()`] which returns the engine together with the
/// "other ends" of the ring buffers that the UI thread should hold:
///
/// ```ignore
/// let (engine, cmd_tx, event_rx) = AudioEngine::new();
/// // Move `engine` to the audio thread.
/// // Keep `cmd_tx` and `event_rx` on the UI thread.
/// ```
pub struct AudioEngine {
    transport: Transport,
    /// Legacy single-audio field for backward compatibility.
    audio: Option<Arc<DecodedAudio>>,
    /// Multi-track state.
    tracks: Vec<EngineTrack>,
    /// One-shot sample preview, bypasses transport and soloed/muted state.
    preview: Option<PreviewVoice>,
    sample_rate: u32,
    cmd_rx: Consumer<EngineCommand>,
    event_tx: Producer<EngineEvent>,
    /// Set when process_multitrack split the block at the arrangement
    /// loop boundary; suppresses the post-advance discontinuity flush
    /// for that block (segment-2 notes are legitimately sounding).
    split_wrap_handled: bool,
}

/// Dedicated single-voice preview channel used by the sample browser.
/// Plays `audio` start-to-end irrespective of transport state and is
/// interrupted whenever a new preview starts.
struct PreviewVoice {
    audio: Arc<DecodedAudio>,
    position: u64,
}

impl AudioEngine {
    /// Create a new audio engine.
    ///
    /// Returns `(engine, command_producer, event_consumer)`.  The caller
    /// should move `engine` to the audio thread and keep the producer /
    /// consumer on the UI thread.
    pub fn new() -> (Self, Producer<EngineCommand>, Consumer<EngineEvent>) {
        let (cmd_tx, cmd_rx) = RingBuffer::<EngineCommand>::new(RING_BUFFER_CAPACITY);
        let (event_tx, event_rx) = RingBuffer::<EngineEvent>::new(RING_BUFFER_CAPACITY);

        let engine = Self {
            transport: Transport::new(),
            audio: None,
            tracks: Vec::new(),
            preview: None,
            sample_rate: 44100,
            cmd_rx,
            event_tx,
            split_wrap_handled: false,
        };

        (engine, cmd_tx, event_rx)
    }

    /// Process one audio callback worth of data.
    ///
    /// `output` is an interleaved stereo buffer (`[L0, R0, L1, R1, ...]`)
    /// that must be filled with audio.  `channels` is the number of
    /// interleaved channels (typically 2).
    ///
    /// This method is **lock-free and allocation-free**.  It:
    /// 1. Drains all pending commands from the ring buffer.
    /// 2. Zeros the output buffer.
    /// 3. If tracks exist: renders each → applies gain/pan → sums into output.
    /// 4. Otherwise: falls back to legacy single-audio path.
    /// 5. Sends metering and position events to the UI thread.
    pub fn process(&mut self, output: &mut [f32], channels: usize) {
        // ---- 1. Drain commands ------------------------------------------
        self.drain_commands();

        let frames = output.len().checked_div(channels).unwrap_or(0);

        // ---- 2. Zero output buffer --------------------------------------
        output.iter_mut().for_each(|s| *s = 0.0);

        if !self.tracks.is_empty() {
            // ---- 3. Multi-track rendering path --------------------------
            self.process_multitrack(output, frames, channels);
        } else {
            // ---- 4. Legacy single-audio path ----------------------------
            self.process_legacy(output, frames, channels);
        }

        // ---- 4.5 Preview channel (bypasses transport) -------------------
        self.process_preview(output, frames, channels);

        // ---- 5. Advance transport and send events -----------------------
        let was_playing = self.transport.is_playing();
        let pos_before = self.transport.position();
        let new_pos = self.transport.advance(frames as u64);

        // Discontinuous jump NOT already handled by the loop-boundary
        // split (auto-stop at project end, loop region moved behind
        // the playhead): kill sounding notes, otherwise they hang
        // forever because the note schedulers only reason about
        // adjacent positions. When the split ran, notes started
        // after the wrap are legitimately sounding and must survive.
        if was_playing
            && !self.split_wrap_handled
            && new_pos != pos_before.saturating_add(frames as u64)
        {
            for track in &mut self.tracks {
                track.flush_notes();
            }
        }
        self.split_wrap_handled = false;

        // Position event.
        let _ = self.event_tx.push(EngineEvent::PlaybackPosition(new_pos));

        // Master metering event.
        let meters = metering::calculate_meters(output, channels);
        let _ = self.event_tx.push(EngineEvent::Metering {
            peak_l: meters.peak_l,
            peak_r: meters.peak_r,
            rms_l: meters.rms_l,
            rms_r: meters.rms_r,
        });
    }

    /// Read the current transport (for inspection / testing).
    pub fn transport(&self) -> &Transport {
        &self.transport
    }

    /// Read the currently loaded audio (for inspection / testing).
    pub fn audio(&self) -> Option<&Arc<DecodedAudio>> {
        self.audio.as_ref()
    }

    /// Read the tracks (for inspection / testing).
    pub fn tracks(&self) -> &[EngineTrack] {
        &self.tracks
    }

    // ------------------------------------------------------------------
    // Private helpers
    // ------------------------------------------------------------------

    /// Multi-track rendering: render each track, apply gain/pan, sum into output.
    ///
    /// When the arrangement loop boundary falls inside this block the
    /// work is split into two segments around it. Without the split,
    /// the block renders linearly past the loop end and the next
    /// block starts after the loop start, so note-ons in the skipped
    /// window (e.g. a note right at the loop start) never fire.
    fn process_multitrack(&mut self, output: &mut [f32], frames: usize, channels: usize) {
        if !self.transport.is_playing() {
            // Stopped transport still renders instruments so
            // auditioned notes (piano-roll keys, drum pads) and
            // plugin-queued events are audible, like any DAW.
            self.render_idle_instruments(output, frames, channels);
            return;
        }

        let pos = self.transport.position();
        if let Some((loop_start, loop_end)) = self.transport.active_loop_region() {
            if pos < loop_end && pos + frames as u64 > loop_end {
                let first = (loop_end - pos) as usize;
                let rest = frames - first;
                self.render_multitrack_segment(
                    &mut output[..first * channels],
                    pos,
                    first,
                    channels,
                );
                // Kill anything sounding across the boundary, then
                // continue sample-accurately from the loop start.
                for track in &mut self.tracks {
                    track.flush_notes();
                }
                if rest > 0 {
                    self.render_multitrack_segment(
                        &mut output[first * channels..],
                        loop_start,
                        rest,
                        channels,
                    );
                }
                self.split_wrap_handled = true;
                return;
            }
        }
        self.render_multitrack_segment(output, pos, frames, channels);
    }

    fn render_multitrack_segment(
        &mut self,
        output: &mut [f32],
        pos: u64,
        frames: usize,
        channels: usize,
    ) {
        let has_solo = any_solo(&self.tracks);

        for track_idx in 0..self.tracks.len() {
            let track = &mut self.tracks[track_idx];

            // Skip muted tracks always
            if track.mute {
                let _ = self.event_tx.push(EngineEvent::TrackMeter {
                    track_id: track.id,
                    peak_l: 0.0,
                    peak_r: 0.0,
                });
                continue;
            }

            // If any track is soloed, skip non-soloed tracks
            if has_solo && !track.solo {
                let _ = self.event_tx.push(EngineEvent::TrackMeter {
                    track_id: track.id,
                    peak_l: 0.0,
                    peak_r: 0.0,
                });
                continue;
            }

            let loop_region = self.transport.active_loop_region();
            let rendered = if track.instrument.is_some() {
                let tempo_map = TempoMap::new(self.transport.bpm(), self.sample_rate);
                track.render_instrument(pos, frames, channels, &tempo_map)
            } else {
                track.render(pos, frames, channels, loop_region)
            };

            if rendered {
                track.process_effects(frames, channels);
            }

            if !rendered {
                let _ = self.event_tx.push(EngineEvent::TrackMeter {
                    track_id: track.id,
                    peak_l: 0.0,
                    peak_r: 0.0,
                });
                continue;
            }

            let gain = track.gain;
            let (pan_l, pan_r) = equal_power_pan(track.pan);
            let track_id = track.id;
            let buf_size = frames * channels;

            // Apply gain and pan, sum into output
            let mut track_peak_l: f32 = 0.0;
            let mut track_peak_r: f32 = 0.0;

            for frame in 0..frames {
                for ch in 0..channels {
                    let idx = frame * channels + ch;
                    if idx >= buf_size {
                        break;
                    }
                    let sample = track.mix_buffer[idx] * gain;
                    let panned = if channels >= 2 {
                        if ch == 0 {
                            sample * pan_l
                        } else if ch == 1 {
                            sample * pan_r
                        } else {
                            sample
                        }
                    } else {
                        sample
                    };

                    output[idx] += panned;

                    // Track per-channel peaks
                    if ch == 0 {
                        track_peak_l = track_peak_l.max(panned.abs());
                    } else if ch == 1 {
                        track_peak_r = track_peak_r.max(panned.abs());
                    }
                }
            }

            let _ = self.event_tx.push(EngineEvent::TrackMeter {
                track_id,
                peak_l: track_peak_l,
                peak_r: track_peak_r,
            });
        }
    }

    /// Render the preview voice into the output buffer on top of whatever
    /// the main graph produced. Bypasses transport, solo, and mute; a
    /// `StartPreview` command during playback simply overlays the preview.
    fn process_preview(&mut self, output: &mut [f32], frames: usize, channels: usize) {
        let Some(preview) = self.preview.as_mut() else {
            return;
        };
        let audio_channels = preview.audio.num_channels();
        let audio_frames = preview.audio.num_frames();
        if audio_channels == 0 || audio_frames == 0 {
            self.preview = None;
            return;
        }

        let start = preview.position as usize;
        let mut consumed = 0usize;
        for frame in 0..frames {
            let source = start + frame;
            if source >= audio_frames {
                break;
            }
            for ch in 0..channels {
                let sample = if ch < audio_channels {
                    preview.audio.sample(ch, source)
                } else {
                    preview.audio.sample(audio_channels - 1, source)
                };
                output[frame * channels + ch] += sample;
            }
            consumed += 1;
        }

        preview.position = preview.position.saturating_add(consumed as u64);
        if preview.position as usize >= audio_frames {
            self.preview = None;
        }
    }

    /// Legacy single-audio rendering path (Phase 1 compatibility).
    fn process_legacy(&mut self, output: &mut [f32], frames: usize, channels: usize) {
        if !self.transport.is_playing() {
            return;
        }

        if let Some(ref audio) = self.audio {
            let pos = self.transport.position();
            let audio_channels = audio.num_channels();

            for frame in 0..frames {
                let sample_idx = pos as usize + frame;

                for ch in 0..channels {
                    let sample = if ch < audio_channels {
                        audio.sample(ch, sample_idx)
                    } else if audio_channels > 0 {
                        audio.sample(audio_channels - 1, sample_idx)
                    } else {
                        0.0
                    };
                    output[frame * channels + ch] = sample;
                }
            }
        }
    }

    /// Drain all pending commands from the ring buffer without blocking.
    fn drain_commands(&mut self) {
        while let Ok(cmd) = self.cmd_rx.pop() {
            match cmd {
                EngineCommand::Play => {
                    self.transport.play();
                    let _ = self.event_tx.push(EngineEvent::PlaybackStarted);
                }
                EngineCommand::Stop => {
                    self.transport.stop();
                    for track in &mut self.tracks {
                        track.flush_notes();
                    }
                    let _ = self.event_tx.push(EngineEvent::PlaybackStopped);
                }
                EngineCommand::Seek(pos) => {
                    self.transport.seek(pos);
                    for track in &mut self.tracks {
                        track.flush_notes();
                    }
                }
                EngineCommand::SetBpm(bpm) => {
                    self.transport.set_bpm(bpm);
                }
                EngineCommand::LoadAudio(audio) => {
                    let len = audio.num_frames() as u64;
                    self.audio = Some(audio);
                    self.transport.set_audio_length(Some(len));
                }
                EngineCommand::UnloadAudio => {
                    self.audio = None;
                    self.transport.set_audio_length(None);
                    self.transport.stop();
                    let _ = self.event_tx.push(EngineEvent::PlaybackStopped);
                }
                // -- Multi-track commands --
                EngineCommand::AddTrack(id, _name) => {
                    self.tracks.push(EngineTrack::new(id));
                    self.recalculate_audio_length();
                }
                EngineCommand::RemoveTrack(id) => {
                    if let Some(pos) = self.tracks.iter().position(|t| t.id == id) {
                        let mut track = self.tracks.remove(pos);
                        for slot in track.effects.drain(..) {
                            self.dispose_effect(slot.effect);
                        }
                        if let Some(instrument) = track.instrument.take() {
                            self.dispose_instrument(instrument);
                        }
                    }
                    self.recalculate_audio_length();
                }
                EngineCommand::ReorderTracks(order) => {
                    self.tracks.sort_by_key(|t| {
                        order
                            .iter()
                            .position(|id| *id == t.id)
                            .unwrap_or(usize::MAX)
                    });
                }
                EngineCommand::AddClip {
                    track_id,
                    clip_id,
                    audio,
                    position,
                    source_offset,
                    duration,
                    loop_enabled,
                    loop_start,
                    loop_end,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        track.clips.push(EngineClip {
                            id: clip_id,
                            audio,
                            position,
                            source_offset,
                            duration,
                            loop_enabled,
                            loop_start,
                            loop_end,
                        });
                    }
                    self.recalculate_audio_length();
                }
                EngineCommand::RemoveClip(track_id, clip_id) => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        track.clips.retain(|c| c.id != clip_id);
                    }
                    self.recalculate_audio_length();
                }
                EngineCommand::ReplaceClipAudio {
                    track_id,
                    clip_id,
                    audio,
                    duration,
                    source_offset,
                    loop_start,
                    loop_end,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                            clip.audio = audio;
                            clip.duration = duration;
                            clip.source_offset = source_offset;
                            clip.loop_start = loop_start;
                            clip.loop_end = loop_end;
                        }
                    }
                    self.recalculate_audio_length();
                }
                EngineCommand::MoveClip {
                    track_id,
                    clip_id,
                    new_position,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                            clip.position = new_position;
                        }
                    }
                    self.recalculate_audio_length();
                }
                EngineCommand::SetTrackGain(id, gain) => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == id) {
                        track.gain = gain;
                    }
                }
                EngineCommand::SetTrackPan(id, pan) => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == id) {
                        track.pan = pan.clamp(0.0, 1.0);
                    }
                }
                EngineCommand::SetTrackMute(id, mute) => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == id) {
                        track.mute = mute;
                    }
                }
                EngineCommand::SetTrackSolo(id, solo) => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == id) {
                        track.solo = solo;
                    }
                }

                // -- Infrastructure --
                EngineCommand::SetSampleRate(sr) => {
                    self.sample_rate = sr;
                }

                // -- Effects --
                EngineCommand::AddEffect {
                    track_id,
                    effect_id,
                    effect_type,
                    position,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        let effect = create_effect(effect_type, self.sample_rate as f32);
                        let slot = EffectSlot {
                            id: effect_id,
                            effect,
                            bypass: false,
                        };
                        if let Some(pos) = position {
                            let idx = pos.min(track.effects.len());
                            track.effects.insert(idx, slot);
                        } else {
                            track.effects.push(slot);
                        }
                    }
                }
                EngineCommand::RemoveEffect(track_id, effect_id) => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        if let Some(pos) = track.effects.iter().position(|e| e.id == effect_id) {
                            let slot = track.effects.remove(pos);
                            self.dispose_effect(slot.effect);
                        }
                    }
                }
                EngineCommand::SetEffectParam {
                    track_id,
                    effect_id,
                    param_index,
                    value,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        if let Some(slot) = track.effects.iter_mut().find(|e| e.id == effect_id) {
                            slot.effect.set_param(param_index, value);
                        }
                    }
                }
                EngineCommand::SetEffectBypass {
                    track_id,
                    effect_id,
                    bypass,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        if let Some(slot) = track.effects.iter_mut().find(|e| e.id == effect_id) {
                            slot.bypass = bypass;
                        }
                    }
                }
                EngineCommand::MoveEffect {
                    track_id,
                    effect_id,
                    new_index,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        if let Some(old_idx) = track.effects.iter().position(|e| e.id == effect_id)
                        {
                            let slot = track.effects.remove(old_idx);
                            let idx = new_index.min(track.effects.len());
                            track.effects.insert(idx, slot);
                        }
                    }
                }

                // -- Instrument tracks --
                EngineCommand::AddInstrumentTrack(id, _name, kind) => {
                    let mut track = EngineTrack::new(id);
                    track.instrument = Some(create_instrument(kind, self.sample_rate as f32));
                    self.tracks.push(track);
                    self.recalculate_audio_length();
                }
                EngineCommand::AddMidiTrack(id, _name) => {
                    self.tracks.push(EngineTrack::new(id));
                    self.recalculate_audio_length();
                }
                EngineCommand::SetTrackInstrument(track_id, kind) => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        let old = track
                            .instrument
                            .replace(create_instrument(kind, self.sample_rate as f32));
                        if let Some(old) = old {
                            self.dispose_instrument(old);
                        }
                    }
                }
                EngineCommand::RemoveTrackInstrument(track_id) => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        if let Some(instrument) = track.instrument.take() {
                            self.dispose_instrument(instrument);
                        }
                    }
                }
                EngineCommand::SetNoteClipDuration {
                    track_id,
                    clip_id,
                    duration_beats,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                            clip.duration_beats = duration_beats;
                        }
                        track.flush_notes();
                    }
                }
                EngineCommand::AddNoteClip {
                    track_id,
                    clip_id,
                    position_beats,
                    duration_beats,
                    loop_enabled,
                    loop_start_beats,
                    loop_end_beats,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        track.note_clips.push(EngineNoteClip {
                            id: clip_id,
                            position_beats,
                            duration_beats,
                            notes: Vec::new(),
                            loop_enabled,
                            loop_start_beats,
                            loop_end_beats,
                        });
                    }
                }
                EngineCommand::RemoveNoteClip(track_id, clip_id) => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        track.note_clips.retain(|c| c.id != clip_id);
                        // Sounding notes get their note-offs from the
                        // clip's schedule; without the clip they hang
                        // forever.
                        track.flush_notes();
                    }
                }
                EngineCommand::MoveNoteClip {
                    track_id,
                    clip_id,
                    new_position_beats,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                            clip.position_beats = new_position_beats;
                        }
                    }
                }
                EngineCommand::AddNote {
                    track_id,
                    clip_id,
                    note,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                            clip.notes.push(note);
                        }
                    }
                }
                EngineCommand::RemoveNote {
                    track_id,
                    clip_id,
                    note_index,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                            if note_index < clip.notes.len() {
                                clip.notes.remove(note_index);
                            }
                        }
                        track.flush_notes();
                    }
                }
                EngineCommand::EditNote {
                    track_id,
                    clip_id,
                    note_index,
                    note,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                            if note_index < clip.notes.len() {
                                clip.notes[note_index] = note;
                            }
                        }
                        track.flush_notes();
                    }
                }
                EngineCommand::SetInstrumentParam {
                    track_id,
                    param_index,
                    value,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        if let Some(ref mut instrument) = track.instrument {
                            instrument.set_param(param_index, value);
                        }
                    }
                }
                EngineCommand::LoadSamplerSample {
                    track_id,
                    sample,
                    sample_name,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        if let Some(ref mut instrument) = track.instrument {
                            instrument.load_sample(sample, sample_name);
                        }
                    }
                }
                EngineCommand::LoadDrumRackPadSample {
                    track_id,
                    pad_index,
                    sample,
                    sample_name,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        if let Some(ref mut instrument) = track.instrument {
                            instrument.load_drum_pad_sample(pad_index, sample, sample_name);
                        }
                    }
                }
                EngineCommand::ClearDrumRackPad {
                    track_id,
                    pad_index,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        if let Some(ref mut instrument) = track.instrument {
                            instrument.clear_drum_pad(pad_index);
                        }
                    }
                }
                EngineCommand::SetDrumRackPadState {
                    track_id,
                    pad_index,
                    state,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        if let Some(ref mut instrument) = track.instrument {
                            instrument.set_drum_pad_state(pad_index, state);
                        }
                    }
                }

                // -- Clip looping --
                EngineCommand::SetArrangementLoop(enabled) => {
                    self.transport.set_loop_enabled(enabled);
                }
                EngineCommand::SetArrangementLoopRegion { start, end } => {
                    self.transport.set_loop_region(start, end);
                }
                EngineCommand::SetClipLoop {
                    track_id,
                    clip_id,
                    enabled,
                    loop_start,
                    loop_end,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                            clip.loop_enabled = enabled;
                            clip.loop_start = loop_start;
                            clip.loop_end = loop_end;
                        }
                    }
                }
                EngineCommand::SetNoteClipLoop {
                    track_id,
                    clip_id,
                    enabled,
                    loop_start_beats,
                    loop_end_beats,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                            clip.loop_enabled = enabled;
                            clip.loop_start_beats = loop_start_beats;
                            clip.loop_end_beats = loop_end_beats;
                        }
                        track.flush_notes();
                    }
                }

                // -- Preview --
                EngineCommand::StartPreview(audio) => {
                    self.preview = Some(PreviewVoice { audio, position: 0 });
                }
                EngineCommand::StopPreview => {
                    self.preview = None;
                }

                // -- External MIDI input --
                EngineCommand::ExternalNoteOn {
                    track_id,
                    pitch,
                    velocity,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        if let Some(instrument) = track.instrument.as_mut() {
                            instrument.note_on(pitch, velocity);
                        }
                    }
                }
                EngineCommand::ExternalNoteOff { track_id, pitch } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        if let Some(instrument) = track.instrument.as_mut() {
                            instrument.note_off(pitch);
                        }
                    }
                }

                // -- External plugins --
                EngineCommand::AddPluginEffect {
                    track_id,
                    effect_id,
                    effect,
                    position,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        let slot = EffectSlot {
                            id: effect_id,
                            effect,
                            bypass: false,
                        };
                        if let Some(pos) = position {
                            let idx = pos.min(track.effects.len());
                            track.effects.insert(idx, slot);
                        } else {
                            track.effects.push(slot);
                        }
                    }
                }
                EngineCommand::AuditionNote {
                    track_id,
                    pitch,
                    velocity,
                    on,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        if let Some(instrument) = track.instrument.as_mut() {
                            if on {
                                instrument.note_on(pitch, velocity);
                            } else {
                                instrument.note_off(pitch);
                            }
                        }
                    }
                }
                EngineCommand::SetPluginInstrument {
                    track_id,
                    instrument,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        if let Some(old) = track.instrument.replace(instrument) {
                            self.dispose_instrument(old);
                        }
                    }
                }
            }
        }
    }

    /// Recalculate transport audio length from all track clips.
    /// Render instruments with no clip scheduling (transport stopped)
    /// so auditioned notes sound. Effects and gain/pan still apply.
    fn render_idle_instruments(&mut self, output: &mut [f32], frames: usize, channels: usize) {
        let has_solo = any_solo(&self.tracks);
        for track in &mut self.tracks {
            if track.instrument.is_none() || track.mute || (has_solo && !track.solo) {
                continue;
            }
            if !track.render_instrument_idle(frames, channels) {
                continue;
            }
            track.process_effects(frames, channels);
            let gain = track.gain;
            let (pan_l, pan_r) = equal_power_pan(track.pan);
            let buf_size = frames * channels;
            for frame in 0..frames {
                for ch in 0..channels {
                    let idx = frame * channels + ch;
                    if idx >= buf_size {
                        break;
                    }
                    let sample = track.mix_buffer[idx] * gain;
                    let panned = if channels == 2 {
                        if ch == 0 {
                            sample * pan_l
                        } else {
                            sample * pan_r
                        }
                    } else {
                        sample
                    };
                    output[idx] += panned;
                }
            }
        }
    }

    /// Hand a removed device back to the UI thread for teardown. If
    /// the event ring is full (should never happen for these rare
    /// events) the device is leaked rather than destroyed here:
    /// plugin destructors are wildly RT-unsafe (dlclose, COM, JUCE).
    fn dispose_effect(&mut self, effect: Box<dyn vibez_dsp::effect::AudioEffect>) {
        if let Err(rtrb::PushError::Full(item)) =
            self.event_tx
                .push(crate::events::EngineEvent::DisposeEffect(
                    crate::events::DisposalCell::new(effect),
                ))
        {
            std::mem::forget(item);
        }
    }

    /// See [`Self::dispose_effect`].
    fn dispose_instrument(&mut self, instrument: Box<dyn vibez_instruments::Instrument>) {
        if let Err(rtrb::PushError::Full(item)) =
            self.event_tx
                .push(crate::events::EngineEvent::DisposeInstrument(
                    crate::events::DisposalCell::new(instrument),
                ))
        {
            std::mem::forget(item);
        }
    }

    fn recalculate_audio_length(&mut self) {
        let total = calculate_total_length(&self.tracks);
        if total > 0 {
            self.transport.set_audio_length(Some(total));
        } else if self.audio.is_none() {
            // Only clear audio length if no legacy audio is loaded
            self.transport.set_audio_length(None);
        }
    }
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "engine_stuck_note_tests.rs"]
mod stuck_note_tests;
