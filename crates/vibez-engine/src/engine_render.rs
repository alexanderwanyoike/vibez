//! Arrange, Section, and stopped-instrument rendering paths.

use super::*;

impl AudioEngine {
    /// Multi-track rendering: render each track, apply gain/pan, sum into output.
    ///
    /// When the arrangement loop boundary falls inside this block the
    /// work is split into two segments around it. Without the split,
    /// the block renders linearly past the loop end and the next
    /// block starts after the loop start, so note-ons in the skipped
    /// window (e.g. a note right at the loop start) never fire.
    pub(super) fn process_multitrack(
        &mut self,
        output: &mut [f32],
        frames: usize,
        channels: usize,
    ) {
        if !self.transport.is_playing() {
            // Stopped transport still renders instruments so
            // auditioned notes (piano-roll keys, drum pads) and
            // plugin-queued events are audible, like any DAW.
            self.render_idle_instruments(output, frames, channels, self.performance_position);
            return;
        }

        if self.process_section_record_count_in(output, frames, channels) {
            return;
        }

        if self.active_section.is_some() {
            if frames > 0 {
                self.start_section_record_if_due(
                    self.performance_position
                        .saturating_add(frames as u64)
                        .saturating_sub(1),
                );
            }
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

    pub(super) fn render_multitrack_segment(
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
                let section = self.active_section;
                let mut on_repeat = |trigger: crate::note_repeat::NoteRepeatTrigger| {
                    let section_position = section.map(|active| {
                        section_record::section_sample_for_performance(
                            pos,
                            repeat_pos,
                            trigger.effective_at_samples,
                            active.length_samples,
                        )
                    });
                    let canonical_section_position = section.map(|active| {
                        section_record::section_sample_for_performance(
                            pos,
                            repeat_pos,
                            trigger.canonical_at_samples,
                            active.length_samples,
                        )
                    });
                    let _ = event_tx.push(EngineEvent::NoteRepeated {
                        track_id,
                        pitch: trigger.pitch,
                        velocity: trigger.velocity,
                        rate: trigger.rate,
                        effective_at_samples: trigger.effective_at_samples,
                        canonical_at_samples: trigger.canonical_at_samples,
                        section_id: section.map(|active| active.section_id),
                        section_position_samples: section_position,
                        canonical_section_position_samples: canonical_section_position,
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
    pub(super) fn process_legacy(&mut self, output: &mut [f32], frames: usize, channels: usize) {
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
    pub(super) fn render_idle_instruments(
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
            let beat = if self.transport.bpm() > 0.0 {
                self.transport.position() as f64 * self.transport.bpm()
                    / (self.sample_rate as f64 * 60.0)
            } else {
                0.0
            };
            track.apply_automation(beat);
            let has_signal = if track.instrument.is_some() {
                let tempo_map = TempoMap::new(self.transport.bpm(), self.sample_rate);
                let track_id = track.id;
                let event_tx = &mut self.event_tx;
                let mut on_repeat = |trigger: crate::note_repeat::NoteRepeatTrigger| {
                    let _ = event_tx.push(EngineEvent::NoteRepeated {
                        track_id,
                        pitch: trigger.pitch,
                        velocity: trigger.velocity,
                        rate: trigger.rate,
                        effective_at_samples: trigger.effective_at_samples,
                        canonical_at_samples: trigger.canonical_at_samples,
                        section_id: None,
                        section_position_samples: None,
                        canonical_section_position_samples: None,
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
}
