use std::sync::Arc;

use rtrb::{Consumer, Producer, RingBuffer};
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::constants::RING_BUFFER_CAPACITY;
use vibez_core::id::{SectionId, TrackId};
use vibez_core::perform::{GrooveProfile, SwingAmount};

use vibez_core::time::TempoMap;
use vibez_dsp::factory::create_effect;
use vibez_instruments::create_instrument;

use crate::commands::{AuditionSync, EngineCommand};
use crate::engine_audition::{audition_fade_frames, next_audition_boundary, AuditionBus};
use crate::events::EngineEvent;
use crate::metering;
use crate::mixer::{
    any_solo, equal_power_pan, EffectSlot, EngineClip, EngineNoteClip, EngineTrack,
    InstrumentRenderContext,
};
use crate::note_repeat::{NoteRepeatClock, NoteRepeatStart};
use crate::playback_source::{calculate_total_length, PreparedSectionPlaybackSource};
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
    active_section: Option<ActiveSectionPlayback>,
    queued_section: Option<QueuedSectionPlayback>,
    /// When a transition lands partway through the current callback, only
    /// these frames advance the newly active Section's local playhead.
    section_advance_override: Option<u64>,
    arrangement_audio_length: Option<u64>,
    /// Project Swing is engine state because generated-event consumers share
    /// it while existing playback remains untouched.
    project_swing: SwingAmount,
    /// Continues Note Repeat timing while transport is stopped. While playing,
    /// this is kept aligned with the absolute transport position.
    performance_position: u64,
    /// Vibez extends MPC Note Repeat to stopped transport. The first held pad
    /// establishes this shared musical downbeat until the last repeat stops.
    stopped_note_repeat_anchor: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
struct ActiveSectionPlayback {
    section_id: SectionId,
    position_samples: u64,
    length_samples: u64,
    looping: bool,
}

struct QueuedSectionPlayback {
    prepared: Box<PreparedSectionPlaybackSource>,
    effective_at_samples: u64,
}

impl AudioEngine {
    /// Timing profile compiled into V1 generated-event scheduling.
    pub const fn groove_profile() -> GrooveProfile {
        GrooveProfile::Mpc2000XlV1
    }

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
            active_section: None,
            queued_section: None,
            section_advance_override: None,
            arrangement_audio_length: None,
            project_swing: SwingAmount::default(),
            performance_position: 0,
            stopped_note_repeat_anchor: None,
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
        self.audition.process(
            output,
            frames,
            channels,
            &self.transport,
            self.sample_rate,
            &mut self.event_tx,
        );

        // ---- 5. Advance transport and send events -----------------------
        let was_playing = self.transport.is_playing();
        let pos_before = self.transport.position();
        let new_pos = if self.active_section.is_some() {
            self.transport.advance_unbounded(frames as u64)
        } else {
            self.transport.advance(frames as u64)
        };
        self.performance_position = if was_playing {
            new_pos
        } else {
            self.performance_position.saturating_add(frames as u64)
        };

        let mut section_ended = false;
        if was_playing {
            if let Some(section) = self.active_section.as_mut() {
                let section_frames = self
                    .section_advance_override
                    .take()
                    .unwrap_or(frames as u64);
                let advanced = section.position_samples.saturating_add(section_frames);
                if section.looping && section.length_samples > 0 {
                    section.position_samples = advanced % section.length_samples;
                } else {
                    section.position_samples = advanced.min(section.length_samples);
                    if advanced >= section.length_samples {
                        self.transport.stop();
                        let _ = self.event_tx.push(EngineEvent::PlaybackStopped);
                        section_ended = true;
                    }
                }
                let _ = self.event_tx.push(EngineEvent::SectionPlaybackPosition {
                    section_id: section.section_id,
                    position_samples: section.position_samples,
                });
            }
        }
        if section_ended {
            self.active_section = None;
            self.transport
                .set_audio_length(self.arrangement_audio_length);
        }

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

    fn reschedule_note_repeats(&mut self) {
        let position = self.performance_position;
        let bpm = self.transport.bpm();
        let sample_rate = self.sample_rate;
        let project_swing = self.project_swing;
        for track in &mut self.tracks {
            track.reschedule_note_repeats(position, bpm, sample_rate, project_swing);
        }
    }

    fn has_active_note_repeats(&self) -> bool {
        self.tracks.iter().any(EngineTrack::has_active_note_repeats)
    }

    fn playing_note_repeat_anchor(&self) -> u64 {
        self.active_section
            .map(|section| {
                self.performance_position
                    .saturating_sub(section.position_samples)
            })
            .unwrap_or(0)
    }

    fn reanchor_note_repeats(&mut self, anchor_sample: u64, after_sample: u64) {
        let bpm = self.transport.bpm();
        let sample_rate = self.sample_rate;
        let project_swing = self.project_swing;
        for track in &mut self.tracks {
            track.reanchor_note_repeats(
                anchor_sample,
                after_sample,
                bpm,
                sample_rate,
                project_swing,
            );
        }
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
            self.render_idle_instruments(output, frames, channels, self.performance_position);
            return;
        }

        if self.active_section.is_some() {
            self.process_section_multitrack(output, frames, channels);
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
                    pos,
                    first,
                    channels,
                    self.transport.active_loop_region(),
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
                        loop_start,
                        rest,
                        channels,
                        self.transport.active_loop_region(),
                    );
                }
                self.split_wrap_handled = true;
                return;
            }
        }
        self.render_multitrack_segment(
            output,
            pos,
            pos,
            frames,
            channels,
            self.transport.active_loop_region(),
        );
    }

    fn render_multitrack_segment(
        &mut self,
        output: &mut [f32],
        pos: u64,
        repeat_pos: u64,
        frames: usize,
        channels: usize,
        loop_region: Option<(u64, u64)>,
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

            let rendered = if track.instrument.is_some() {
                let tempo_map = TempoMap::new(self.transport.bpm(), self.sample_rate);
                let track_id = track.id;
                let event_tx = &mut self.event_tx;
                let mut on_repeat = |trigger: crate::note_repeat::NoteRepeatTrigger| {
                    let _ = event_tx.push(EngineEvent::NoteRepeated {
                        track_id,
                        pitch: trigger.pitch,
                        velocity: trigger.velocity,
                        effective_at_samples: trigger.effective_at_samples,
                    });
                };
                track.render_instrument(
                    InstrumentRenderContext {
                        pos,
                        repeat_pos,
                        frames,
                        channels,
                        tempo_map: &tempo_map,
                        project_swing: self.project_swing,
                    },
                    &mut on_repeat,
                )
            } else {
                track.render(pos, frames, channels, loop_region)
            };

            // Always run the shared device chain. A silent new Section stops
            // source material while already-produced effect tails continue.
            track.process_effects(frames, channels);
            let rendered = rendered
                || track.mix_buffer[..frames * channels]
                    .iter()
                    .any(|sample| *sample != 0.0);

            if rendered && self.spectrum_track == Some(track.id) {
                push_spectrum(
                    &mut self.spectrum_tx,
                    &track.mix_buffer[..frames * channels],
                    channels,
                );
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

    /// Recalculate transport audio length from all track clips.
    /// Render instruments with no clip scheduling (transport stopped)
    /// so auditioned notes sound. Effects and gain/pan still apply.
    fn render_idle_instruments(
        &mut self,
        output: &mut [f32],
        frames: usize,
        channels: usize,
        repeat_pos: u64,
    ) {
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
                let tempo_map = TempoMap::new(self.transport.bpm(), self.sample_rate);
                let track_id = track.id;
                let event_tx = &mut self.event_tx;
                let mut on_repeat = |trigger: crate::note_repeat::NoteRepeatTrigger| {
                    let _ = event_tx.push(EngineEvent::NoteRepeated {
                        track_id,
                        pitch: trigger.pitch,
                        velocity: trigger.velocity,
                        effective_at_samples: trigger.effective_at_samples,
                    });
                };
                track.render_instrument_idle(
                    repeat_pos,
                    frames,
                    channels,
                    &tempo_map,
                    self.project_swing,
                    &mut on_repeat,
                )
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
        let total = calculate_total_length(
            self.tracks
                .iter()
                .map(|track| track.playback_source.as_ref()),
            samples_per_beat,
        );
        self.arrangement_audio_length = if total > 0 {
            Some(total)
        } else {
            self.audio.as_ref().map(|audio| audio.num_frames() as u64)
        };
        if self.active_section.is_none() {
            self.transport
                .set_audio_length(self.arrangement_audio_length);
        }
    }
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

#[path = "engine_drain_commands.rs"]
mod drain_commands;

#[path = "engine_section_queue.rs"]
mod section_queue;

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "engine_stuck_note_tests.rs"]
mod stuck_note_tests;

#[cfg(test)]
#[path = "engine_mute_event_tests.rs"]
mod mute_event_tests;

#[cfg(test)]
#[path = "engine_note_repeat_tests.rs"]
mod note_repeat_tests;

#[cfg(test)]
#[path = "engine_clip_groove_tests.rs"]
mod clip_groove_tests;

#[cfg(test)]
#[path = "engine_section_playback_tests.rs"]
mod section_playback_tests;

#[cfg(test)]
#[path = "engine_section_queue_tests.rs"]
mod section_queue_tests;
