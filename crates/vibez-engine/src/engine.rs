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

        let frames = if channels > 0 {
            output.len() / channels
        } else {
            0
        };

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
mod tests {
    use super::*;
    use vibez_core::audio_buffer::DecodedAudio;
    use vibez_core::id::{ClipId, TrackId};

    /// Helper to create a simple stereo decoded audio with a known pattern.
    fn make_test_audio(frames: usize) -> Arc<DecodedAudio> {
        let left: Vec<f32> = (0..frames).map(|i| (i as f32) / (frames as f32)).collect();
        let right: Vec<f32> = (0..frames)
            .map(|i| -((i as f32) / (frames as f32)))
            .collect();
        Arc::new(DecodedAudio {
            channels: vec![left, right],
            sample_rate: 44_100,
        })
    }

    fn make_constant_audio(frames: usize, value: f32) -> Arc<DecodedAudio> {
        Arc::new(DecodedAudio {
            channels: vec![vec![value; frames], vec![value; frames]],
            sample_rate: 44_100,
        })
    }

    #[test]
    fn new_returns_ring_buffer_endpoints() {
        let (engine, _cmd_tx, _event_rx) = AudioEngine::new();
        assert!(!engine.transport().is_playing());
        assert!(engine.audio().is_none());
    }

    #[test]
    fn process_outputs_silence_when_stopped() {
        let (mut engine, _cmd_tx, _event_rx) = AudioEngine::new();
        let mut buf = vec![999.0f32; 512];
        engine.process(&mut buf, 2);

        assert!(buf.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn play_command_starts_transport() {
        let (mut engine, mut cmd_tx, mut event_rx) = AudioEngine::new();

        cmd_tx.push(EngineCommand::Play).unwrap();

        let mut buf = vec![0.0f32; 8];
        engine.process(&mut buf, 2);

        assert!(engine.transport().is_playing());

        // Should have received PlaybackStarted event.
        let mut found_started = false;
        while let Ok(event) = event_rx.pop() {
            if event == EngineEvent::PlaybackStarted {
                found_started = true;
            }
        }
        assert!(found_started);
    }

    #[test]
    fn stop_command_stops_transport() {
        let (mut engine, mut cmd_tx, mut event_rx) = AudioEngine::new();

        cmd_tx.push(EngineCommand::Play).unwrap();
        cmd_tx.push(EngineCommand::Stop).unwrap();

        let mut buf = vec![0.0f32; 8];
        engine.process(&mut buf, 2);

        assert!(!engine.transport().is_playing());

        let mut found_stopped = false;
        while let Ok(event) = event_rx.pop() {
            if event == EngineEvent::PlaybackStopped {
                found_stopped = true;
            }
        }
        assert!(found_stopped);
    }

    #[test]
    fn load_audio_and_play() {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let audio = make_test_audio(1024);

        cmd_tx
            .push(EngineCommand::LoadAudio(audio.clone()))
            .unwrap();
        cmd_tx.push(EngineCommand::Play).unwrap();

        let mut buf = vec![0.0f32; 16]; // 8 frames stereo
        engine.process(&mut buf, 2);

        // The output should contain the first 8 frames of the test audio.
        for frame in 0..8 {
            let expected_l = audio.sample(0, frame);
            let expected_r = audio.sample(1, frame);
            let actual_l = buf[frame * 2];
            let actual_r = buf[frame * 2 + 1];
            assert!(
                (actual_l - expected_l).abs() < 1e-6,
                "frame {frame} L: expected {expected_l} got {actual_l}"
            );
            assert!(
                (actual_r - expected_r).abs() < 1e-6,
                "frame {frame} R: expected {expected_r} got {actual_r}"
            );
        }

        // Transport should have advanced by 8 frames.
        assert_eq!(engine.transport().position(), 8);
    }

    #[test]
    fn seek_then_play() {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let audio = make_test_audio(1024);

        cmd_tx
            .push(EngineCommand::LoadAudio(audio.clone()))
            .unwrap();
        cmd_tx.push(EngineCommand::Seek(100)).unwrap();
        cmd_tx.push(EngineCommand::Play).unwrap();

        let mut buf = vec![0.0f32; 8]; // 4 frames stereo
        engine.process(&mut buf, 2);

        // Should be playing from position 100.
        let expected_l = audio.sample(0, 100);
        assert!((buf[0] - expected_l).abs() < 1e-6);
        assert_eq!(engine.transport().position(), 104);
    }

    #[test]
    fn unload_audio_stops_and_clears() {
        let (mut engine, mut cmd_tx, mut event_rx) = AudioEngine::new();
        let audio = make_test_audio(1024);

        cmd_tx.push(EngineCommand::LoadAudio(audio)).unwrap();
        cmd_tx.push(EngineCommand::Play).unwrap();

        let mut buf = vec![0.0f32; 16];
        engine.process(&mut buf, 2);
        assert!(engine.audio().is_some());

        cmd_tx.push(EngineCommand::UnloadAudio).unwrap();

        let mut buf = vec![0.0f32; 16];
        engine.process(&mut buf, 2);

        assert!(engine.audio().is_none());
        assert!(!engine.transport().is_playing());

        // Drain events and check for PlaybackStopped.
        let mut found_stopped = false;
        while let Ok(event) = event_rx.pop() {
            if event == EngineEvent::PlaybackStopped {
                found_stopped = true;
            }
        }
        assert!(found_stopped);
    }

    #[test]
    fn set_bpm_command() {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        cmd_tx.push(EngineCommand::SetBpm(140.0)).unwrap();

        let mut buf = vec![0.0f32; 8];
        engine.process(&mut buf, 2);

        assert!((engine.transport().bpm() - 140.0).abs() < f64::EPSILON);
    }

    #[test]
    fn metering_events_are_sent() {
        let (mut engine, mut cmd_tx, mut event_rx) = AudioEngine::new();
        let audio = make_test_audio(1024);

        cmd_tx.push(EngineCommand::LoadAudio(audio)).unwrap();
        cmd_tx.push(EngineCommand::Play).unwrap();

        let mut buf = vec![0.0f32; 512];
        engine.process(&mut buf, 2);

        let mut found_metering = false;
        while let Ok(event) = event_rx.pop() {
            if let EngineEvent::Metering { .. } = event {
                found_metering = true;
            }
        }
        assert!(found_metering);
    }

    #[test]
    fn position_events_are_sent() {
        let (mut engine, _cmd_tx, mut event_rx) = AudioEngine::new();

        let mut buf = vec![0.0f32; 64];
        engine.process(&mut buf, 2);

        let mut found_position = false;
        while let Ok(event) = event_rx.pop() {
            if let EngineEvent::PlaybackPosition(pos) = event {
                found_position = true;
                assert_eq!(pos, 0); // transport is stopped, position stays 0
            }
        }
        assert!(found_position);
    }

    #[test]
    fn auto_stop_at_end_of_audio() {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let audio = make_test_audio(16); // only 16 frames

        cmd_tx.push(EngineCommand::LoadAudio(audio)).unwrap();
        cmd_tx.push(EngineCommand::Play).unwrap();

        // Request 32 frames (more than the 16 available).
        let mut buf = vec![0.0f32; 64]; // 32 frames stereo
        engine.process(&mut buf, 2);

        // Transport should have auto-stopped at frame 16.
        assert!(!engine.transport().is_playing());
        assert_eq!(engine.transport().position(), 16);

        // Samples beyond the audio length should be 0 (DecodedAudio::sample
        // returns 0 for out-of-bounds).
        // Frames 16..31 should be silence.
        for frame in 16..32 {
            assert_eq!(buf[frame * 2], 0.0, "frame {frame} L should be 0");
            assert_eq!(buf[frame * 2 + 1], 0.0, "frame {frame} R should be 0");
        }
    }

    #[test]
    fn multiple_process_calls_advance_position() {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let audio = make_test_audio(1024);

        cmd_tx.push(EngineCommand::LoadAudio(audio)).unwrap();
        cmd_tx.push(EngineCommand::Play).unwrap();

        let mut buf = vec![0.0f32; 128]; // 64 frames
        engine.process(&mut buf, 2);
        assert_eq!(engine.transport().position(), 64);

        engine.process(&mut buf, 2);
        assert_eq!(engine.transport().position(), 128);

        engine.process(&mut buf, 2);
        assert_eq!(engine.transport().position(), 192);
    }

    #[test]
    fn mono_audio_to_stereo_output() {
        let mono_audio = Arc::new(DecodedAudio {
            channels: vec![vec![0.5; 64]],
            sample_rate: 44_100,
        });

        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        cmd_tx.push(EngineCommand::LoadAudio(mono_audio)).unwrap();
        cmd_tx.push(EngineCommand::Play).unwrap();

        let mut buf = vec![0.0f32; 16]; // 8 frames stereo
        engine.process(&mut buf, 2);

        // Both channels should get the mono signal.
        for frame in 0..8 {
            assert!((buf[frame * 2] - 0.5).abs() < 1e-6);
            assert!((buf[frame * 2 + 1] - 0.5).abs() < 1e-6);
        }
    }

    #[test]
    fn process_with_zero_length_buffer() {
        let (mut engine, _cmd_tx, _event_rx) = AudioEngine::new();
        let mut buf: Vec<f32> = vec![];
        // Should not panic.
        engine.process(&mut buf, 2);
    }

    // -- Multi-track tests --

    #[test]
    fn add_and_remove_tracks() {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let tid1 = TrackId::new();
        let tid2 = TrackId::new();

        cmd_tx
            .push(EngineCommand::AddTrack(tid1, "Track 1".into()))
            .unwrap();
        cmd_tx
            .push(EngineCommand::AddTrack(tid2, "Track 2".into()))
            .unwrap();

        let mut buf = vec![0.0f32; 8];
        engine.process(&mut buf, 2);
        assert_eq!(engine.tracks().len(), 2);

        cmd_tx.push(EngineCommand::RemoveTrack(tid1)).unwrap();
        engine.process(&mut buf, 2);
        assert_eq!(engine.tracks().len(), 1);
        assert_eq!(engine.tracks()[0].id, tid2);
    }

    #[test]
    fn reorder_tracks() {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let tid1 = TrackId::new();
        let tid2 = TrackId::new();
        let tid3 = TrackId::new();

        cmd_tx
            .push(EngineCommand::AddTrack(tid1, "Track 1".into()))
            .unwrap();
        cmd_tx
            .push(EngineCommand::AddTrack(tid2, "Track 2".into()))
            .unwrap();
        cmd_tx
            .push(EngineCommand::AddTrack(tid3, "Track 3".into()))
            .unwrap();

        let mut buf = vec![0.0f32; 8];
        engine.process(&mut buf, 2);
        assert_eq!(engine.tracks().len(), 3);
        assert_eq!(engine.tracks()[0].id, tid1);
        assert_eq!(engine.tracks()[1].id, tid2);
        assert_eq!(engine.tracks()[2].id, tid3);

        // Reverse the order
        cmd_tx
            .push(EngineCommand::ReorderTracks(vec![tid3, tid2, tid1]))
            .unwrap();
        engine.process(&mut buf, 2);
        assert_eq!(engine.tracks()[0].id, tid3);
        assert_eq!(engine.tracks()[1].id, tid2);
        assert_eq!(engine.tracks()[2].id, tid1);
    }

    #[test]
    fn add_clip_and_play() {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let tid = TrackId::new();
        let cid = ClipId::new();
        let audio = make_constant_audio(100, 0.5);

        cmd_tx
            .push(EngineCommand::AddTrack(tid, "Track 1".into()))
            .unwrap();
        cmd_tx
            .push(EngineCommand::AddClip {
                track_id: tid,
                clip_id: cid,
                audio,
                position: 0,
                source_offset: 0,
                duration: 100,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
            })
            .unwrap();
        cmd_tx.push(EngineCommand::Play).unwrap();

        let mut buf = vec![0.0f32; 16]; // 8 frames
        engine.process(&mut buf, 2);

        // With center pan (0.5), equal power gives ~0.707 on each channel
        let expected = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
        for frame in 0..8 {
            assert!(
                (buf[frame * 2] - expected).abs() < 1e-4,
                "frame {} L: expected {} got {}",
                frame,
                expected,
                buf[frame * 2]
            );
            assert!(
                (buf[frame * 2 + 1] - expected).abs() < 1e-4,
                "frame {} R: expected {} got {}",
                frame,
                expected,
                buf[frame * 2 + 1]
            );
        }
    }

    #[test]
    fn mute_silences_track() {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let tid = TrackId::new();
        let cid = ClipId::new();
        let audio = make_constant_audio(100, 0.8);

        cmd_tx
            .push(EngineCommand::AddTrack(tid, "Track 1".into()))
            .unwrap();
        cmd_tx
            .push(EngineCommand::AddClip {
                track_id: tid,
                clip_id: cid,
                audio,
                position: 0,
                source_offset: 0,
                duration: 100,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
            })
            .unwrap();
        cmd_tx.push(EngineCommand::SetTrackMute(tid, true)).unwrap();
        cmd_tx.push(EngineCommand::Play).unwrap();

        let mut buf = vec![0.0f32; 16];
        engine.process(&mut buf, 2);

        assert!(buf.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn solo_isolates_track() {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let tid1 = TrackId::new();
        let tid2 = TrackId::new();
        let cid1 = ClipId::new();
        let cid2 = ClipId::new();

        let audio1 = make_constant_audio(100, 0.5);
        let audio2 = make_constant_audio(100, 0.3);

        cmd_tx
            .push(EngineCommand::AddTrack(tid1, "Track 1".into()))
            .unwrap();
        cmd_tx
            .push(EngineCommand::AddTrack(tid2, "Track 2".into()))
            .unwrap();
        cmd_tx
            .push(EngineCommand::AddClip {
                track_id: tid1,
                clip_id: cid1,
                audio: audio1,
                position: 0,
                source_offset: 0,
                duration: 100,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
            })
            .unwrap();
        cmd_tx
            .push(EngineCommand::AddClip {
                track_id: tid2,
                clip_id: cid2,
                audio: audio2,
                position: 0,
                source_offset: 0,
                duration: 100,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
            })
            .unwrap();
        // Solo track 1 only
        cmd_tx
            .push(EngineCommand::SetTrackSolo(tid1, true))
            .unwrap();
        cmd_tx.push(EngineCommand::Play).unwrap();

        let mut buf = vec![0.0f32; 16]; // 8 frames
        engine.process(&mut buf, 2);

        // Only track 1 should be audible (0.5 * pan_gain)
        let expected = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
        for frame in 0..8 {
            assert!(
                (buf[frame * 2] - expected).abs() < 1e-4,
                "frame {} L: expected {} got {}",
                frame,
                expected,
                buf[frame * 2]
            );
        }
    }

    #[test]
    fn gain_scaling() {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let tid = TrackId::new();
        let cid = ClipId::new();
        let audio = make_constant_audio(100, 1.0);

        cmd_tx
            .push(EngineCommand::AddTrack(tid, "Track 1".into()))
            .unwrap();
        cmd_tx
            .push(EngineCommand::AddClip {
                track_id: tid,
                clip_id: cid,
                audio,
                position: 0,
                source_offset: 0,
                duration: 100,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
            })
            .unwrap();
        cmd_tx.push(EngineCommand::SetTrackGain(tid, 0.5)).unwrap();
        cmd_tx.push(EngineCommand::Play).unwrap();

        let mut buf = vec![0.0f32; 16];
        engine.process(&mut buf, 2);

        let expected = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
        assert!((buf[0] - expected).abs() < 1e-4);
    }

    #[test]
    fn pan_hard_left() {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let tid = TrackId::new();
        let cid = ClipId::new();
        let audio = make_constant_audio(100, 1.0);

        cmd_tx
            .push(EngineCommand::AddTrack(tid, "Track 1".into()))
            .unwrap();
        cmd_tx
            .push(EngineCommand::AddClip {
                track_id: tid,
                clip_id: cid,
                audio,
                position: 0,
                source_offset: 0,
                duration: 100,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
            })
            .unwrap();
        cmd_tx.push(EngineCommand::SetTrackPan(tid, 0.0)).unwrap();
        cmd_tx.push(EngineCommand::Play).unwrap();

        let mut buf = vec![0.0f32; 16];
        engine.process(&mut buf, 2);

        // Left channel should be full (1.0 * 1.0), right should be ~0
        assert!((buf[0] - 1.0).abs() < 1e-4, "left should be ~1.0");
        assert!(buf[1].abs() < 1e-4, "right should be ~0.0");
    }

    #[test]
    fn multi_track_summing() {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let tid1 = TrackId::new();
        let tid2 = TrackId::new();
        let cid1 = ClipId::new();
        let cid2 = ClipId::new();

        let audio1 = make_constant_audio(100, 0.3);
        let audio2 = make_constant_audio(100, 0.4);

        cmd_tx
            .push(EngineCommand::AddTrack(tid1, "Track 1".into()))
            .unwrap();
        cmd_tx
            .push(EngineCommand::AddTrack(tid2, "Track 2".into()))
            .unwrap();
        cmd_tx
            .push(EngineCommand::AddClip {
                track_id: tid1,
                clip_id: cid1,
                audio: audio1,
                position: 0,
                source_offset: 0,
                duration: 100,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
            })
            .unwrap();
        cmd_tx
            .push(EngineCommand::AddClip {
                track_id: tid2,
                clip_id: cid2,
                audio: audio2,
                position: 0,
                source_offset: 0,
                duration: 100,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
            })
            .unwrap();
        cmd_tx.push(EngineCommand::Play).unwrap();

        let mut buf = vec![0.0f32; 16];
        engine.process(&mut buf, 2);

        // Both at center pan: each channel = (0.3 + 0.4) * FRAC_1_SQRT_2
        let expected = (0.3 + 0.4) * std::f32::consts::FRAC_1_SQRT_2;
        assert!(
            (buf[0] - expected).abs() < 1e-3,
            "expected {} got {}",
            expected,
            buf[0]
        );
    }

    #[test]
    fn legacy_compat_with_tracks_present() {
        // When tracks exist, legacy audio is ignored (multi-track path is used)
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let legacy_audio = make_constant_audio(100, 0.9);
        let tid = TrackId::new();

        cmd_tx.push(EngineCommand::LoadAudio(legacy_audio)).unwrap();
        cmd_tx
            .push(EngineCommand::AddTrack(tid, "Track 1".into()))
            .unwrap();
        cmd_tx.push(EngineCommand::Play).unwrap();

        let mut buf = vec![0.0f32; 16];
        engine.process(&mut buf, 2);

        // Track has no clips, so output should be silent despite legacy audio being loaded
        assert!(buf.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn per_track_metering_events() {
        let (mut engine, mut cmd_tx, mut event_rx) = AudioEngine::new();
        let tid = TrackId::new();
        let cid = ClipId::new();
        let audio = make_constant_audio(100, 0.5);

        cmd_tx
            .push(EngineCommand::AddTrack(tid, "Track 1".into()))
            .unwrap();
        cmd_tx
            .push(EngineCommand::AddClip {
                track_id: tid,
                clip_id: cid,
                audio,
                position: 0,
                source_offset: 0,
                duration: 100,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
            })
            .unwrap();
        cmd_tx.push(EngineCommand::Play).unwrap();

        let mut buf = vec![0.0f32; 16];
        engine.process(&mut buf, 2);

        let mut found_track_meter = false;
        while let Ok(event) = event_rx.pop() {
            if let EngineEvent::TrackMeter {
                track_id,
                peak_l,
                peak_r,
            } = event
            {
                if track_id == tid {
                    found_track_meter = true;
                    assert!(peak_l > 0.0);
                    assert!(peak_r > 0.0);
                }
            }
        }
        assert!(found_track_meter);
    }

    #[test]
    fn transport_auto_stop_multitrack() {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let tid = TrackId::new();
        let cid = ClipId::new();
        let audio = make_constant_audio(16, 0.5);

        cmd_tx
            .push(EngineCommand::AddTrack(tid, "Track 1".into()))
            .unwrap();
        cmd_tx
            .push(EngineCommand::AddClip {
                track_id: tid,
                clip_id: cid,
                audio,
                position: 0,
                source_offset: 0,
                duration: 16,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
            })
            .unwrap();
        cmd_tx.push(EngineCommand::Play).unwrap();

        // Request 32 frames, but only 16 frames of audio exist
        let mut buf = vec![0.0f32; 64];
        engine.process(&mut buf, 2);

        assert!(!engine.transport().is_playing());
        assert_eq!(engine.transport().position(), 16);
    }

    #[test]
    fn move_clip_changes_position() {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let tid = TrackId::new();
        let cid = ClipId::new();
        let audio = make_constant_audio(100, 0.5);

        cmd_tx
            .push(EngineCommand::AddTrack(tid, "Track 1".into()))
            .unwrap();
        cmd_tx
            .push(EngineCommand::AddClip {
                track_id: tid,
                clip_id: cid,
                audio,
                position: 0,
                source_offset: 0,
                duration: 100,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
            })
            .unwrap();
        cmd_tx
            .push(EngineCommand::MoveClip {
                track_id: tid,
                clip_id: cid,
                new_position: 50,
            })
            .unwrap();

        let mut buf = vec![0.0f32; 8];
        engine.process(&mut buf, 2);

        // Clip is now at position 50, engine should recognize this
        assert_eq!(engine.tracks()[0].clips[0].position, 50);
    }

    #[test]
    fn add_clip_with_loop_plays_looped_audio() {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let tid = TrackId::new();
        let cid = ClipId::new();
        let audio = make_constant_audio(100, 0.5);

        cmd_tx
            .push(EngineCommand::AddTrack(tid, "Track 1".into()))
            .unwrap();
        cmd_tx
            .push(EngineCommand::AddClip {
                track_id: tid,
                clip_id: cid,
                audio,
                position: 0,
                source_offset: 0,
                duration: 200,
                loop_enabled: true,
                loop_start: 0,
                loop_end: 100,
            })
            .unwrap();
        cmd_tx.push(EngineCommand::Play).unwrap();

        // Process 200 frames (source is only 100 frames, but loop should fill)
        let mut buf = vec![0.0f32; 400]; // 200 frames stereo
        engine.process(&mut buf, 2);

        // Frame 150 should have non-zero audio (looped region)
        let pan_gain = std::f32::consts::FRAC_1_SQRT_2;
        let expected = 0.5 * pan_gain;
        assert!(
            (buf[150 * 2] - expected).abs() < 1e-4,
            "frame 150 L: expected ~{expected}, got {}",
            buf[150 * 2]
        );
    }

    #[test]
    fn resize_clip_preserves_loop_state() {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let tid = TrackId::new();
        let cid = ClipId::new();
        let audio = make_constant_audio(100, 0.5);

        cmd_tx
            .push(EngineCommand::AddTrack(tid, "Track 1".into()))
            .unwrap();
        // Add clip without loop
        cmd_tx
            .push(EngineCommand::AddClip {
                track_id: tid,
                clip_id: cid,
                audio: audio.clone(),
                position: 0,
                source_offset: 0,
                duration: 100,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
            })
            .unwrap();
        // Enable loop via SetClipLoop
        cmd_tx
            .push(EngineCommand::SetClipLoop {
                track_id: tid,
                clip_id: cid,
                enabled: true,
                loop_start: 0,
                loop_end: 100,
            })
            .unwrap();
        // Process to apply commands
        let mut buf = vec![0.0f32; 8];
        engine.process(&mut buf, 2);

        // Simulate resize: Remove + Add with loop fields
        cmd_tx.push(EngineCommand::RemoveClip(tid, cid)).unwrap();
        cmd_tx
            .push(EngineCommand::AddClip {
                track_id: tid,
                clip_id: cid,
                audio,
                position: 0,
                source_offset: 0,
                duration: 200,
                loop_enabled: true,
                loop_start: 0,
                loop_end: 100,
            })
            .unwrap();
        cmd_tx.push(EngineCommand::Seek(0)).unwrap();
        cmd_tx.push(EngineCommand::Play).unwrap();

        let mut buf = vec![0.0f32; 400]; // 200 frames
        engine.process(&mut buf, 2);

        // Frame 150 (in looped region) should have audio
        let pan_gain = std::f32::consts::FRAC_1_SQRT_2;
        let expected = 0.5 * pan_gain;
        assert!(
            (buf[150 * 2] - expected).abs() < 1e-4,
            "frame 150 L after resize: expected ~{expected}, got {}",
            buf[150 * 2]
        );
    }

    #[test]
    fn preview_plays_even_when_transport_stopped() {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let audio = make_constant_audio(64, 0.4);

        cmd_tx.push(EngineCommand::StartPreview(audio)).unwrap();

        // Transport is stopped: regular tracks would produce silence,
        // but the preview voice should still render.
        let mut buf = vec![0.0f32; 16];
        engine.process(&mut buf, 2);

        let peak = buf.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        assert!(peak > 0.3, "preview should be audible: peak {peak}");
    }

    #[test]
    fn stop_preview_silences_playback() {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let audio = make_constant_audio(1024, 0.5);

        cmd_tx.push(EngineCommand::StartPreview(audio)).unwrap();
        cmd_tx.push(EngineCommand::StopPreview).unwrap();

        let mut buf = vec![0.0f32; 16];
        engine.process(&mut buf, 2);
        assert!(buf.iter().all(|s| s.abs() < 1e-6));
    }

    #[test]
    fn preview_auto_completes_at_end_of_audio() {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let audio = make_constant_audio(8, 0.6);

        cmd_tx.push(EngineCommand::StartPreview(audio)).unwrap();

        // First 4 frames: audible
        let mut buf = vec![0.0f32; 8];
        engine.process(&mut buf, 2);
        assert!(buf.iter().any(|s| s.abs() > 0.5));

        // Next 16 frames: past end of 8-frame audio. Preview auto-clears.
        let mut buf = vec![0.0f32; 32];
        engine.process(&mut buf, 2);
        // First 8 samples of buf correspond to frames 4-7 of preview (audible).
        // The remaining should be silence.
        let tail = &buf[16..];
        assert!(tail.iter().all(|s| s.abs() < 1e-6));
    }

    #[test]
    fn starting_new_preview_interrupts_previous() {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let a = make_constant_audio(1024, 0.8);
        let b = make_constant_audio(1024, 0.2);

        cmd_tx.push(EngineCommand::StartPreview(a)).unwrap();
        let mut buf = vec![0.0f32; 16];
        engine.process(&mut buf, 2);
        assert!(buf[0].abs() > 0.7);

        cmd_tx.push(EngineCommand::StartPreview(b)).unwrap();
        let mut buf = vec![0.0f32; 16];
        engine.process(&mut buf, 2);
        assert!(buf[0].abs() < 0.3);
    }

    #[test]
    fn note_clip_loop_renders() {
        use vibez_core::midi::{InstrumentKind, MidiNote};

        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let tid = TrackId::new();
        let cid = ClipId::new();

        // Set sample rate first so synth is initialized properly
        cmd_tx.push(EngineCommand::SetSampleRate(44_100)).unwrap();
        cmd_tx
            .push(EngineCommand::AddInstrumentTrack(
                tid,
                "Synth 1".into(),
                InstrumentKind::SubtractiveSynth,
            ))
            .unwrap();
        // Add note clip: 2 beats, looped over [0, 2) with total duration 4 beats
        cmd_tx
            .push(EngineCommand::AddNoteClip {
                track_id: tid,
                clip_id: cid,
                position_beats: 0.0,
                duration_beats: 4.0,
                loop_enabled: true,
                loop_start_beats: 0.0,
                loop_end_beats: 2.0,
            })
            .unwrap();
        // Add a note at beat 0, 1 beat long
        cmd_tx
            .push(EngineCommand::AddNote {
                track_id: tid,
                clip_id: cid,
                note: MidiNote {
                    pitch: 60,
                    velocity: 100,
                    start_beat: 0.0,
                    duration_beats: 1.0,
                },
            })
            .unwrap();
        cmd_tx.push(EngineCommand::Play).unwrap();

        // At 120 BPM, 1 beat = 22050 samples (44100 / 2)
        // Process enough frames to reach beat 3 (in the looped region)
        // 3 beats = 66150 samples
        let frames = 66_150;
        let mut buf = vec![0.0f32; frames * 2];
        engine.process(&mut buf, 2);

        // Check that there's audio in the looped region (around beat 2-3)
        // At beat 2.5 = sample 55125, the looped note should trigger again
        let looped_region_start = 44_100; // beat 2.0
        let looped_region_end = 66_150; // beat 3.0
        let has_audio_in_loop = buf[looped_region_start * 2..looped_region_end * 2]
            .iter()
            .any(|&s| s.abs() > 1e-6);
        assert!(
            has_audio_in_loop,
            "Expected synth audio in looped region (beat 2-3)"
        );
    }
}

#[cfg(test)]
mod stuck_note_tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use vibez_core::id::{ClipId, TrackId};
    use vibez_core::midi::{InstrumentKind, MidiNote};

    /// Instrument that records every note event it receives.
    struct SpyInstrument {
        events: Arc<Mutex<Vec<(bool, u8)>>>,
    }

    impl vibez_instruments::Instrument for SpyInstrument {
        fn instrument_kind(&self) -> InstrumentKind {
            InstrumentKind::SubtractiveSynth
        }
        fn param_descriptors(&self) -> &'static [vibez_core::effect::ParamDescriptor] {
            &[]
        }
        fn set_param(&mut self, _index: usize, _value: f32) -> bool {
            false
        }
        fn get_param(&self, _index: usize) -> f32 {
            0.0
        }
        fn note_on(&mut self, pitch: u8, _velocity: u8) {
            self.events.lock().unwrap().push((true, pitch));
        }
        fn note_off(&mut self, pitch: u8) {
            self.events.lock().unwrap().push((false, pitch));
        }
        fn render(&mut self, _buffer: &mut [f32], _channels: usize) {}
        fn reset(&mut self) {}
    }

    /// Engine with one MIDI track holding a held note (0..8 beats in
    /// an 8-beat clip, no clip loop) and a spy instrument.
    fn engine_with_held_note() -> (
        AudioEngine,
        rtrb::Producer<EngineCommand>,
        Arc<Mutex<Vec<(bool, u8)>>>,
        TrackId,
    ) {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let events = Arc::new(Mutex::new(Vec::new()));
        let tid = TrackId::new();
        let cid = ClipId::new();
        cmd_tx
            .push(EngineCommand::AddMidiTrack(tid, "midi".into()))
            .unwrap();
        cmd_tx
            .push(EngineCommand::SetPluginInstrument {
                track_id: tid,
                instrument: Box::new(SpyInstrument {
                    events: Arc::clone(&events),
                }),
            })
            .unwrap();
        cmd_tx
            .push(EngineCommand::AddNoteClip {
                track_id: tid,
                clip_id: cid,
                position_beats: 0.0,
                duration_beats: 8.0,
                loop_enabled: false,
                loop_start_beats: 0.0,
                loop_end_beats: 0.0,
            })
            .unwrap();
        cmd_tx
            .push(EngineCommand::AddNote {
                track_id: tid,
                clip_id: cid,
                note: MidiNote {
                    pitch: 60,
                    velocity: 100,
                    start_beat: 0.0,
                    duration_beats: 8.0,
                },
            })
            .unwrap();
        cmd_tx.push(EngineCommand::Play).unwrap();
        let mut buf = vec![0.0f32; 512 * 2];
        engine.process(&mut buf, 2); // drains commands, starts the note
        assert!(
            events.lock().unwrap().contains(&(true, 60)),
            "note-on should have fired"
        );
        (engine, cmd_tx, events, tid)
    }

    /// Reproduce the dogfood report: 8-beat clip, 1-beat note at the
    /// start, clip loop on, arrangement loop over the same 2 bars.
    /// Every note-on must be closed by a note-off before the next
    /// note-on of the same pitch (otherwise the voice piles up /
    /// drones).
    fn assert_no_hanging_notes(events: &[(bool, u8)]) {
        let mut sounding = false;
        for (i, &(is_on, pitch)) in events.iter().enumerate() {
            assert_eq!(pitch, 60);
            if is_on {
                assert!(!sounding, "note-on #{i} while already sounding: {events:?}");
                sounding = true;
            } else {
                sounding = false;
            }
        }
    }

    fn run_scenario(
        clip_loop: (bool, f64, f64),
        arr_loop: Option<(u64, u64)>,
        note_dur: f64,
        blocks: usize,
    ) -> Vec<(bool, u8)> {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let events = Arc::new(Mutex::new(Vec::new()));
        let tid = TrackId::new();
        let cid = ClipId::new();
        cmd_tx
            .push(EngineCommand::AddMidiTrack(tid, "midi".into()))
            .unwrap();
        cmd_tx
            .push(EngineCommand::SetPluginInstrument {
                track_id: tid,
                instrument: Box::new(SpyInstrument {
                    events: Arc::clone(&events),
                }),
            })
            .unwrap();
        cmd_tx
            .push(EngineCommand::AddNoteClip {
                track_id: tid,
                clip_id: cid,
                position_beats: 0.0,
                duration_beats: 8.0,
                loop_enabled: clip_loop.0,
                loop_start_beats: clip_loop.1,
                loop_end_beats: clip_loop.2,
            })
            .unwrap();
        cmd_tx
            .push(EngineCommand::AddNote {
                track_id: tid,
                clip_id: cid,
                note: MidiNote {
                    pitch: 60,
                    velocity: 100,
                    start_beat: 0.0,
                    duration_beats: note_dur,
                },
            })
            .unwrap();
        if let Some((start, end)) = arr_loop {
            cmd_tx
                .push(EngineCommand::SetArrangementLoopRegion { start, end })
                .unwrap();
            cmd_tx
                .push(EngineCommand::SetArrangementLoop(true))
                .unwrap();
        }
        cmd_tx.push(EngineCommand::Play).unwrap();
        let mut buf = vec![0.0f32; 512 * 2];
        for _ in 0..blocks {
            engine.process(&mut buf, 2);
        }
        let out = events.lock().unwrap().clone();
        out
    }

    #[test]
    fn dogfood_clip_loop_one_bar_in_two_bar_clip() {
        // Clip loop repeats the first bar inside the 2-bar clip.
        // 120 BPM default: samples_per_beat = 44100 * 60/120 = 22050.
        // 8 beats = 176400 samples. Arrangement loop the same 2 bars.
        let events = run_scenario((true, 0.0, 4.0), Some((0, 176_400)), 1.0, 1500);
        assert!(
            events.iter().filter(|e| e.0).count() >= 4,
            "expected several note-ons: {events:?}"
        );
        assert_no_hanging_notes(&events);
    }

    #[test]
    fn dogfood_full_clip_loop_with_arrangement_loop() {
        let events = run_scenario((true, 0.0, 8.0), Some((0, 176_400)), 1.0, 1500);
        assert!(
            events.iter().filter(|e| e.0).count() >= 2,
            "expected repeated note-ons: {events:?}"
        );
        assert_no_hanging_notes(&events);
    }

    #[test]
    fn dogfood_note_spanning_clip_loop_boundary() {
        // Note as long as the loop region: off lands exactly on the wrap.
        let events = run_scenario((true, 0.0, 4.0), Some((0, 176_400)), 4.0, 1500);
        assert_no_hanging_notes(&events);
    }

    #[test]
    fn instrument_params_change_the_sound() {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let tid = TrackId::new();
        cmd_tx
            .push(EngineCommand::AddMidiTrack(tid, "midi".into()))
            .unwrap();
        cmd_tx
            .push(EngineCommand::SetTrackInstrument(
                tid,
                vibez_core::midi::InstrumentKind::SubtractiveSynth,
            ))
            .unwrap();
        cmd_tx
            .push(EngineCommand::AuditionNote {
                track_id: tid,
                pitch: 60,
                velocity: 100,
                on: true,
            })
            .unwrap();
        let mut open_buf = vec![0.0f32; 2048 * 2];
        engine.process(&mut open_buf, 2);
        engine.process(&mut open_buf, 2);

        // Slam the filter shut (param 5 = cutoff in Hz) and compare.
        cmd_tx
            .push(EngineCommand::SetInstrumentParam {
                track_id: tid,
                param_index: 5,
                value: 60.0,
            })
            .unwrap();
        let mut closed_buf = vec![0.0f32; 2048 * 2];
        engine.process(&mut closed_buf, 2);
        engine.process(&mut closed_buf, 2);

        let rms = |b: &[f32]| (b.iter().map(|s| s * s).sum::<f32>() / b.len() as f32).sqrt();
        let open_rms = rms(&open_buf);
        let closed_rms = rms(&closed_buf);
        assert!(open_rms > 0.0);
        assert!(
            closed_rms < open_rms * 0.7,
            "60 Hz cutoff must audibly darken/quiet a C4 saw: open={open_rms} closed={closed_rms}"
        );
    }

    #[test]
    fn audition_sounds_while_transport_stopped() {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let tid = TrackId::new();
        cmd_tx
            .push(EngineCommand::AddMidiTrack(tid, "midi".into()))
            .unwrap();
        cmd_tx
            .push(EngineCommand::SetTrackInstrument(
                tid,
                vibez_core::midi::InstrumentKind::SubtractiveSynth,
            ))
            .unwrap();
        cmd_tx
            .push(EngineCommand::AuditionNote {
                track_id: tid,
                pitch: 60,
                velocity: 100,
                on: true,
            })
            .unwrap();
        // Transport NEVER started: idle rendering must still sound.
        let mut buf = vec![0.0f32; 512 * 2];
        engine.process(&mut buf, 2);
        engine.process(&mut buf, 2);
        assert!(
            buf.iter().any(|&s| s.abs() > 1e-6),
            "auditioned note must be audible while stopped"
        );
        cmd_tx
            .push(EngineCommand::AuditionNote {
                track_id: tid,
                pitch: 60,
                velocity: 100,
                on: false,
            })
            .unwrap();
        // Long tail drain: after release the synth decays to silence.
        for _ in 0..200 {
            engine.process(&mut buf, 2);
        }
        assert!(
            buf.iter().all(|&s| s.abs() < 1e-4),
            "note must decay after audition release"
        );
    }

    #[test]
    fn removing_clip_kills_sounding_notes() {
        let (mut engine, mut cmd_tx, events, tid) = engine_with_held_note();
        // Clip id is unknown here; removing ALL note clips on the
        // track exercises the same path.
        let cid = {
            // recover clip id from engine state
            engine.tracks()[0].note_clips[0].id
        };
        cmd_tx
            .push(EngineCommand::RemoveNoteClip(tid, cid))
            .unwrap();
        let mut buf = vec![0.0f32; 512 * 2];
        engine.process(&mut buf, 2);
        assert!(
            events.lock().unwrap().contains(&(false, 60)),
            "deleting the clip must kill its sounding notes"
        );
    }

    #[test]
    fn arrangement_loop_wrap_kills_sounding_notes() {
        let (mut engine, mut cmd_tx, events, _tid) = engine_with_held_note();
        // Tight arrangement loop so the transport wraps mid-note.
        cmd_tx
            .push(EngineCommand::SetArrangementLoopRegion {
                start: 0,
                end: 2048,
            })
            .unwrap();
        cmd_tx
            .push(EngineCommand::SetArrangementLoop(true))
            .unwrap();

        let mut buf = vec![0.0f32; 512 * 2];
        for _ in 0..8 {
            engine.process(&mut buf, 2); // crosses 2048 and wraps
        }
        assert!(
            events.lock().unwrap().contains(&(false, 60)),
            "wrap must send a note-off for the sounding note, got {:?}",
            events.lock().unwrap()
        );
    }

    #[test]
    fn stop_kills_sounding_notes() {
        let (mut engine, mut cmd_tx, events, _tid) = engine_with_held_note();
        cmd_tx.push(EngineCommand::Stop).unwrap();
        let mut buf = vec![0.0f32; 512 * 2];
        engine.process(&mut buf, 2);
        assert!(
            events.lock().unwrap().contains(&(false, 60)),
            "stop must send a note-off for the sounding note"
        );
    }

    #[test]
    fn seek_kills_sounding_notes() {
        let (mut engine, mut cmd_tx, events, _tid) = engine_with_held_note();
        cmd_tx.push(EngineCommand::Seek(96_000)).unwrap();
        let mut buf = vec![0.0f32; 512 * 2];
        engine.process(&mut buf, 2);
        assert!(
            events.lock().unwrap().contains(&(false, 60)),
            "seek must send a note-off for the sounding note"
        );
    }
}
