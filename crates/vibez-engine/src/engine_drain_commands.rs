//! Audio-thread engine command dispatch (`AudioEngine::drain_commands`).
//! Split from engine.rs. Runs in the audio callback: keep allocation-free and lock-free.

use super::*;

impl AudioEngine {
    /// Drain all pending commands from the ring buffer without blocking.
    pub(super) fn drain_commands(&mut self) {
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
                EngineCommand::StartAudition {
                    audio,
                    sync,
                    looped,
                } => {
                    let fade_frames = audition_fade_frames(self.sample_rate);
                    if self.transport.is_playing() && sync != AuditionSync::Off {
                        let beats = if sync == AuditionSync::Bar { 4 } else { 1 };
                        let target = next_audition_boundary(
                            self.transport.position(),
                            self.transport.bpm(),
                            self.sample_rate,
                            beats,
                        );
                        self.audition.queue(audio, target, fade_frames, looped);
                        let _ = self.event_tx.push(EngineEvent::AuditionQueued);
                    } else {
                        self.audition.start(audio, fade_frames, looped);
                        let _ = self.event_tx.push(EngineEvent::AuditionStarted);
                    }
                }
                EngineCommand::StopAudition => {
                    // A queued-only audition has no voice to fade, so
                    // process_audition would never emit a terminal
                    // event; emit it here or a buffered AuditionQueued
                    // polled after the stop leaves the UI stuck QUEUED.
                    let queued_only = self.audition.queued.is_some()
                        && self.audition.active.is_none()
                        && !self.audition.has_outgoing();
                    self.audition.stop(audition_fade_frames(self.sample_rate));
                    if queued_only {
                        let _ = self.event_tx.push(EngineEvent::AuditionStopped);
                    }
                }
                EngineCommand::SetAuditionGain(gain) => {
                    self.audition.gain = gain.clamp(0.0, 2.0);
                }
                EngineCommand::SetAuditionLoop(looped) => {
                    self.audition.set_looped(looped);
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
}
