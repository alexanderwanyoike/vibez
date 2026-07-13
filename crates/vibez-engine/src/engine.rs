use std::sync::Arc;

use rtrb::{Consumer, Producer, RingBuffer};
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::constants::RING_BUFFER_CAPACITY;
use vibez_core::id::TrackId;

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

/// Capacity of the UI spectrum-analyser sample ring: roughly a third
/// of a second of mono audio at 48 kHz, ample for a 60 fps drain.
const SPECTRUM_RING_CAPACITY: usize = 16_384;

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
    /// The master bus: a track-shaped channel (gain + effect chain,
    /// no clips or instrument) applied to the summed mix. Addressed
    /// by commands via [`TrackId::MASTER`].
    master: EngineTrack,
    /// Return channels: track-shaped, fed by per-track post-fader
    /// sends, summed into the mix before the master chain.
    buses: Vec<EngineTrack>,
    /// Which track streams samples to the UI spectrum analyser.
    spectrum_track: Option<TrackId>,
    /// Producer side of the spectrum ring (mono samples).
    spectrum_tx: Producer<f32>,
    /// Consumer side, parked here until the UI takes it before the
    /// engine moves to the audio thread.
    spectrum_rx: Option<Consumer<f32>>,
    /// One-shot Browser audition after project master processing.
    audition: AuditionBus,
    sample_rate: u32,
    cmd_rx: Consumer<EngineCommand>,
    event_tx: Producer<EngineEvent>,
    /// Set when process_multitrack split the block at the arrangement
    /// loop boundary; suppresses the post-advance discontinuity flush
    /// for that block (segment-2 notes are legitimately sounding).
    split_wrap_handled: bool,
}

/// Dedicated Browser signal path mixed after project master processing.
struct AuditionBus {
    active: Option<AuditionVoice>,
    outgoing: Option<AuditionVoice>,
    gain: f32,
}

struct AuditionVoice {
    audio: Arc<DecodedAudio>,
    position: u64,
    fade_in_frames: usize,
    fade_out_remaining: Option<usize>,
}

impl AuditionBus {
    fn new() -> Self {
        Self {
            active: None,
            outgoing: None,
            gain: 1.0,
        }
    }

    fn start(&mut self, audio: Arc<DecodedAudio>, fade_frames: usize) {
        if let Some(mut active) = self.active.take() {
            if active.position > 0 {
                active.fade_out_remaining = Some(fade_frames);
                self.outgoing = Some(active);
            }
        }
        self.active = Some(AuditionVoice {
            audio,
            position: 0,
            fade_in_frames: fade_frames,
            fade_out_remaining: None,
        });
    }

    fn stop(&mut self, fade_frames: usize) {
        if let Some(mut active) = self.active.take() {
            if active.position > 0 {
                active.fade_out_remaining = Some(fade_frames);
                self.outgoing = Some(active);
            }
        }
    }
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
        let (spectrum_tx, spectrum_rx) = RingBuffer::<f32>::new(SPECTRUM_RING_CAPACITY);

        let engine = Self {
            transport: Transport::new(),
            audio: None,
            tracks: Vec::new(),
            master: EngineTrack::new(TrackId::MASTER),
            buses: Vec::new(),
            spectrum_track: None,
            spectrum_tx,
            spectrum_rx: Some(spectrum_rx),
            audition: AuditionBus::new(),
            sample_rate: 44100,
            cmd_rx,
            event_tx,
            split_wrap_handled: false,
        };

        (engine, cmd_tx, event_rx)
    }

    /// Take the consumer side of the spectrum-analyser ring. Must be
    /// called on the UI thread before the engine moves to the audio
    /// thread; returns `None` if already taken.
    pub fn take_spectrum_consumer(&mut self) -> Option<Consumer<f32>> {
        self.spectrum_rx.take()
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

        // Bus mixes accumulate from track sends during rendering;
        // start each block from silence.
        for bus in &mut self.buses {
            bus.clear_buffer(frames, channels);
        }

        if !self.tracks.is_empty() {
            // ---- 3. Multi-track rendering path --------------------------
            self.process_multitrack(output, frames, channels);
        } else {
            // ---- 4. Legacy single-audio path ----------------------------
            self.process_legacy(output, frames, channels);
        }

        // ---- 4.3 Buses: each return processes its send mix through
        // its chain (always, so queued plugin params deliver and
        // tails ring out) and sums into the mix ahead of the master.
        let block_beat = if self.transport.is_playing() {
            let bpm = self.transport.bpm();
            if bpm > 0.0 {
                Some(self.transport.position() as f64 * bpm / (self.sample_rate as f64 * 60.0))
            } else {
                None
            }
        } else {
            None
        };
        let has_bus_solo = any_solo(&self.buses);
        for bus_idx in 0..self.buses.len() {
            let bus = &mut self.buses[bus_idx];
            let (bus_auto_gain, bus_auto_pan) = match block_beat {
                Some(beat) => bus.apply_automation(beat),
                None => (None, None),
            };
            let buf_size = frames * channels;
            if bus.mix_buffer.len() < buf_size {
                bus.clear_buffer(frames, channels);
            }
            for slot in &mut bus.effects {
                if !slot.bypass {
                    slot.effect
                        .process(&mut bus.mix_buffer[..buf_size], channels);
                }
            }
            if self.spectrum_track == Some(bus.id) {
                push_spectrum(&mut self.spectrum_tx, &bus.mix_buffer[..buf_size], channels);
            }

            let mut peak_l = 0.0f32;
            let mut peak_r = 0.0f32;
            if !bus.mute && (!has_bus_solo || bus.solo) {
                let gain = bus_auto_gain.unwrap_or(bus.gain);
                // Balance, not equal-power: the send mix is already
                // panned stereo, center must pass at unity.
                let (pan_l, pan_r) = crate::mixer::balance_pan(bus_auto_pan.unwrap_or(bus.pan));
                for frame in 0..frames {
                    for ch in 0..channels {
                        let idx = frame * channels + ch;
                        let sample = bus.mix_buffer[idx] * gain;
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
                        if ch == 0 {
                            peak_l = peak_l.max(panned.abs());
                        } else if ch == 1 {
                            peak_r = peak_r.max(panned.abs());
                        }
                    }
                }
            }
            let bus_id = bus.id;
            let _ = self.event_tx.push(EngineEvent::TrackMeter {
                track_id: bus_id,
                peak_l,
                peak_r,
            });
        }

        // ---- 4.4 Master bus: effect chain + gain over the summed mix.
        // Runs whether or not the transport is playing so queued
        // plugin params are delivered and tails ring out, matching
        // the per-track idle behavior.
        let (master_auto_gain, _) = match block_beat {
            Some(beat) => self.master.apply_automation(beat),
            None => (None, None),
        };
        for slot in &mut self.master.effects {
            if !slot.bypass {
                slot.effect.process(output, channels);
            }
        }
        let master_gain = master_auto_gain.unwrap_or(self.master.gain);
        if (master_gain - 1.0).abs() > f32::EPSILON {
            output.iter_mut().for_each(|s| *s *= master_gain);
        }
        if self.spectrum_track == Some(self.master.id) {
            push_spectrum(&mut self.spectrum_tx, output, channels);
        }

        // ---- 4.5 Audition Bus (post-master, outside project graph) ------
        self.process_audition(output, frames, channels);

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
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn tracks(&self) -> &[EngineTrack] {
        &self.tracks
    }

    // ------------------------------------------------------------------
    // Private helpers
    // ------------------------------------------------------------------

    /// Resolve a command's target channel: the master for
    /// [`TrackId::MASTER`], otherwise a track or a bus. Only used by
    /// commands that make sense on all of them (gain, pan, mute,
    /// effect chain).
    fn channel_mut(&mut self, id: TrackId) -> Option<&mut EngineTrack> {
        if id.is_master() {
            return Some(&mut self.master);
        }
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == id) {
            return Some(track);
        }
        self.buses.iter_mut().find(|b| b.id == id)
    }

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
        let has_track_solo = any_solo(&self.tracks);
        let has_bus_solo = any_solo(&self.buses);

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

            // A soloed return still needs every source to render its
            // sends. Without a return solo, preserve normal track
            // solo filtering.
            if has_track_solo && !track.solo && !has_bus_solo {
                let _ = self.event_tx.push(EngineEvent::TrackMeter {
                    track_id: track.id,
                    peak_l: 0.0,
                    peak_r: 0.0,
                });
                continue;
            }

            // Block-rate automation: evaluate every lane at the
            // segment-start beat. Effect/instrument params apply in
            // place; gain/pan come back as overrides for the sum
            // stage below.
            let beat = {
                let bpm = self.transport.bpm();
                if bpm > 0.0 {
                    pos as f64 * bpm / (self.sample_rate as f64 * 60.0)
                } else {
                    0.0
                }
            };
            let (auto_gain, auto_pan) = track.apply_automation(beat);

            let loop_region = self.transport.active_loop_region();
            let rendered = if track.instrument.is_some() {
                let tempo_map = TempoMap::new(self.transport.bpm(), self.sample_rate);
                track.render_instrument(pos, frames, channels, &tempo_map)
            } else {
                track.render(pos, frames, channels, loop_region)
            };

            if rendered {
                track.process_effects(frames, channels);
                if self.spectrum_track == Some(track.id) {
                    push_spectrum(
                        &mut self.spectrum_tx,
                        &track.mix_buffer[..frames * channels],
                        channels,
                    );
                }
            }

            if !rendered {
                let _ = self.event_tx.push(EngineEvent::TrackMeter {
                    track_id: track.id,
                    peak_l: 0.0,
                    peak_r: 0.0,
                });
                continue;
            }

            let gain = auto_gain.unwrap_or(track.gain);
            let (pan_l, pan_r) = equal_power_pan(auto_pan.unwrap_or(track.pan));
            let track_id = track.id;
            let buf_size = frames * channels;
            let dry_audible = (!has_track_solo && !has_bus_solo) || track.solo;

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

                    if dry_audible {
                        output[idx] += panned;
                    }

                    // Track per-channel peaks
                    if ch == 0 {
                        track_peak_l = track_peak_l.max(panned.abs());
                    } else if ch == 1 {
                        track_peak_r = track_peak_r.max(panned.abs());
                    }
                }
            }

            // Post-fader sends: the same gained/panned signal feeds
            // each bus at its send amount.
            for send_idx in 0..track.sends.len() {
                let (bus_id, amount) = track.sends[send_idx];
                if amount <= 0.0005 {
                    continue;
                }
                if let Some(bus) = self.buses.iter_mut().find(|b| b.id == bus_id) {
                    for frame in 0..frames {
                        for ch in 0..channels {
                            let idx = frame * channels + ch;
                            if idx >= buf_size || idx >= bus.mix_buffer.len() {
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
                            bus.mix_buffer[idx] += panned * amount;
                        }
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

    fn process_audition(&mut self, output: &mut [f32], frames: usize, channels: usize) {
        let had_voice = self.audition.active.is_some() || self.audition.outgoing.is_some();
        let gain = self.audition.gain;
        if self
            .audition
            .outgoing
            .as_mut()
            .is_some_and(|voice| render_audition_voice(voice, output, frames, channels, gain))
        {
            self.audition.outgoing = None;
        }
        if self
            .audition
            .active
            .as_mut()
            .is_some_and(|voice| render_audition_voice(voice, output, frames, channels, gain))
        {
            self.audition.active = None;
        }
        if had_voice && self.audition.active.is_none() && self.audition.outgoing.is_none() {
            let _ = self.event_tx.push(EngineEvent::AuditionStopped);
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
                    self.recalculate_audio_length();
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
                    if let Some(track) = self.channel_mut(id) {
                        track.gain = gain;
                    }
                }
                EngineCommand::SetAutomationLane { track_id, lane } => {
                    if let Some(track) = self.channel_mut(track_id) {
                        match track.automation.iter_mut().find(|l| l.id == lane.id) {
                            Some(existing) => *existing = lane,
                            None => track.automation.push(lane),
                        }
                    }
                }
                EngineCommand::RemoveAutomationLane { track_id, lane_id } => {
                    if let Some(track) = self.channel_mut(track_id) {
                        track.automation.retain(|l| l.id != lane_id);
                    }
                }
                EngineCommand::SetTrackPan(id, pan) => {
                    if let Some(track) = self.channel_mut(id) {
                        track.pan = pan.clamp(0.0, 1.0);
                    }
                }
                EngineCommand::SetTrackMute(id, mute) => {
                    if let Some(track) = self.channel_mut(id) {
                        track.mute = mute;
                    }
                }

                // -- Busses --
                EngineCommand::AddBus(id, _name) => {
                    self.buses.push(EngineTrack::new(id));
                }
                EngineCommand::RemoveBus(id) => {
                    if let Some(pos) = self.buses.iter().position(|b| b.id == id) {
                        let mut bus = self.buses.remove(pos);
                        for slot in bus.effects.drain(..) {
                            self.dispose_effect(slot.effect);
                        }
                    }
                    for track in &mut self.tracks {
                        track.sends.retain(|(bus_id, _)| *bus_id != id);
                        track.automation.retain(|lane| {
                            lane.target
                                != vibez_core::automation::AutomationTarget::Send { bus_id: id }
                        });
                    }
                }
                EngineCommand::SetSend {
                    track_id,
                    bus_id,
                    amount,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        let amount = amount.clamp(0.0, 1.0);
                        match track.sends.iter_mut().find(|(b, _)| *b == bus_id) {
                            Some(send) => send.1 = amount,
                            None => track.sends.push((bus_id, amount)),
                        }
                    }
                }
                EngineCommand::SetTrackSolo(id, solo) => {
                    if let Some(channel) = self.channel_mut(id) {
                        channel.solo = solo;
                    }
                }

                // -- Infrastructure --
                EngineCommand::SetSampleRate(sr) => {
                    self.sample_rate = sr;
                    self.recalculate_audio_length();
                }
                EngineCommand::SetSpectrumTap(target) => {
                    self.spectrum_track = target;
                }

                // -- Effects --
                EngineCommand::AddEffect {
                    track_id,
                    effect_id,
                    effect_type,
                    position,
                } => {
                    let effect = create_effect(effect_type, self.sample_rate as f32);
                    if let Some(track) = self.channel_mut(track_id) {
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
                    let removed = self.channel_mut(track_id).and_then(|track| {
                        track
                            .effects
                            .iter()
                            .position(|e| e.id == effect_id)
                            .map(|pos| track.effects.remove(pos))
                    });
                    if let Some(slot) = removed {
                        self.dispose_effect(slot.effect);
                    }
                }
                EngineCommand::SetEffectParam {
                    track_id,
                    effect_id,
                    param_index,
                    value,
                } => {
                    if let Some(track) = self.channel_mut(track_id) {
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
                    if let Some(track) = self.channel_mut(track_id) {
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
                    if let Some(track) = self.channel_mut(track_id) {
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
                    self.recalculate_audio_length();
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
                    self.recalculate_audio_length();
                }
                EngineCommand::RemoveNoteClip(track_id, clip_id) => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        track.note_clips.retain(|c| c.id != clip_id);
                        // Sounding notes get their note-offs from the
                        // clip's schedule; without the clip they hang
                        // forever.
                        track.flush_notes();
                    }
                    self.recalculate_audio_length();
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
                    self.recalculate_audio_length();
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

                // -- Dedicated Audition Bus --
                EngineCommand::StartAudition(audio) => {
                    self.audition
                        .start(audio, audition_fade_frames(self.sample_rate));
                }
                EngineCommand::StopAudition => {
                    self.audition.stop(audition_fade_frames(self.sample_rate));
                }
                EngineCommand::SetAuditionGain(gain) => {
                    self.audition.gain = gain.clamp(0.0, 2.0);
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
                    if let Some(track) = self.channel_mut(track_id) {
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
        let has_track_solo = any_solo(&self.tracks);
        let has_bus_solo = any_solo(&self.buses);
        for track in &mut self.tracks {
            if track.mute || (has_track_solo && !track.solo && !has_bus_solo) {
                continue;
            }
            // Instruments render so auditioned notes sound; every
            // effect chain keeps processing while stopped so delay
            // and reverb tails ring out and queued plugin parameter
            // changes (knob edits, automation) actually reach the
            // plugin instead of waiting for play.
            let has_signal = if track.instrument.is_some() {
                track.render_instrument_idle(frames, channels)
            } else {
                false
            };
            if !has_signal && track.effects.is_empty() {
                continue;
            }
            if !has_signal && track.instrument.is_none() {
                track.clear_buffer(frames, channels);
            }
            track.process_effects(frames, channels);
            if self.spectrum_track == Some(track.id) {
                push_spectrum(
                    &mut self.spectrum_tx,
                    &track.mix_buffer[..frames * channels],
                    channels,
                );
            }
            let gain = track.gain;
            let (pan_l, pan_r) = equal_power_pan(track.pan);
            let buf_size = frames * channels;
            let dry_audible = (!has_track_solo && !has_bus_solo) || track.solo;
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
                    if dry_audible {
                        output[idx] += panned;
                    }
                }
            }
            // Sends feed buses while stopped too, so auditioned
            // notes reach the returns like in any DAW.
            for send_idx in 0..track.sends.len() {
                let (bus_id, amount) = track.sends[send_idx];
                if amount <= 0.0005 {
                    continue;
                }
                if let Some(bus) = self.buses.iter_mut().find(|b| b.id == bus_id) {
                    for frame in 0..frames {
                        for ch in 0..channels {
                            let idx = frame * channels + ch;
                            if idx >= buf_size || idx >= bus.mix_buffer.len() {
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
                            bus.mix_buffer[idx] += panned * amount;
                        }
                    }
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
        let samples_per_beat = if self.transport.bpm() > 0.0 {
            self.sample_rate as f64 * 60.0 / self.transport.bpm()
        } else {
            0.0
        };
        let total = calculate_total_length(&self.tracks, samples_per_beat);
        if total > 0 {
            self.transport.set_audio_length(Some(total));
        } else if self.audio.is_none() {
            // Only clear audio length if no legacy audio is loaded
            self.transport.set_audio_length(None);
        }
    }
}

const AUDITION_FADE_MS: usize = 5;

fn audition_fade_frames(sample_rate: u32) -> usize {
    ((sample_rate as usize * AUDITION_FADE_MS) / 1_000).max(1)
}

/// Mix one RAW audition voice. Returns true once its source or fade is done.
fn render_audition_voice(
    voice: &mut AuditionVoice,
    output: &mut [f32],
    frames: usize,
    channels: usize,
    bus_gain: f32,
) -> bool {
    let audio_channels = voice.audio.num_channels();
    let audio_frames = voice.audio.num_frames();
    if audio_channels == 0 || audio_frames == 0 || channels == 0 {
        return true;
    }

    for frame in 0..frames {
        let source = voice.position as usize;
        if source >= audio_frames {
            return true;
        }
        let attack = if source < voice.fade_in_frames {
            (source + 1) as f32 / voice.fade_in_frames as f32
        } else {
            1.0
        };
        let remaining_source = audio_frames.saturating_sub(source + 1);
        let natural_release =
            (remaining_source as f32 / voice.fade_in_frames as f32).clamp(0.0, 1.0);
        let commanded_release = match voice.fade_out_remaining {
            Some(remaining) => {
                let envelope = remaining.saturating_sub(1) as f32 / voice.fade_in_frames as f32;
                voice.fade_out_remaining = Some(remaining.saturating_sub(1));
                envelope
            }
            None => 1.0,
        };
        let envelope = attack.min(natural_release) * commanded_release * bus_gain;
        for ch in 0..channels {
            let source_channel = ch.min(audio_channels - 1);
            output[frame * channels + ch] += voice.audio.sample(source_channel, source) * envelope;
        }
        voice.position = voice.position.saturating_add(1);
        if voice.fade_out_remaining == Some(0) {
            return true;
        }
    }

    voice.position as usize >= audio_frames
}

/// Stream a block to the UI spectrum analyser as mono samples.
/// Lock-free and allocation-free; drops samples when the ring is
/// full (the UI drains at 60 fps, so sustained overflow just means
/// the analyser skips audio it would have averaged away anyway).
fn push_spectrum(tx: &mut Producer<f32>, buffer: &[f32], channels: usize) {
    if channels >= 2 {
        for frame in buffer.chunks_exact(channels) {
            let _ = tx.push((frame[0] + frame[1]) * 0.5);
        }
    } else {
        for &s in buffer {
            let _ = tx.push(s);
        }
    }
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "engine_stuck_note_tests.rs"]
mod stuck_note_tests;
