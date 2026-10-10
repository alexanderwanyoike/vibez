//! Audio-thread engine command dispatch (`AudioEngine::drain_commands`).
//! Split from engine.rs. Runs in the audio callback: keep allocation-free and lock-free.

use super::*;

impl AudioEngine {
    /// Drain all pending commands from the ring buffer without blocking.
    pub(super) fn drain_commands(&mut self) {
        self.rendered_callback_frames = 0;
        self.flush_presentation();
        self.continue_capture_replay();
        if self.capture_replay.is_some() {
            return;
        }
        self.return_retired_routing();
        self.flush_retirements();
        self.clean_removed_bus_automation();
        loop {
            if self.capture_replay.is_some() {
                break;
            }
            let presentation_reserve = self.tracks.len().saturating_mul(2).saturating_add(4);
            if self
                .scheduled_presentation
                .capacity()
                .saturating_sub(self.scheduled_presentation.len())
                < presentation_reserve
                || self
                    .section_capture_timing
                    .capacity()
                    .saturating_sub(self.section_capture_timing.len())
                    < 2
            {
                self.fail_presentation();
                break;
            }
            if self.scheduled_presentation.len() + 2 >= self.scheduled_presentation.capacity()
                || self.section_capture_timing.len() + 2 >= self.section_capture_timing.capacity()
                || self.pending_bus_cleanup.is_some()
                || self
                    .pending_retirements
                    .capacity()
                    .saturating_sub(self.pending_retirements.len())
                    < self.cmd_rx.peek().map_or(2, |cmd| cmd.retirement_reserve())
                || !self.channel_retirement.has_capacity()
            {
                break;
            }
            // A stalled UI must retain plan ownership, without callback
            // destruction or unbounded leaked retirement buffers.
            if self.retired_routing.is_some()
                && matches!(
                    self.cmd_rx.peek(),
                    Ok(EngineCommand::SetRouting(_)
                        | EngineCommand::UpdateAutomationRouting(_)
                        | EngineCommand::ResumeDeviceReconfiguration { .. })
                )
            {
                break;
            }
            let Ok(mut cmd) = self.cmd_rx.pop() else {
                break;
            };
            self.prepare_device_edit(&mut cmd);
            if self.retain_device_parameter(&cmd) {
                continue;
            }
            if self.routing.is_some()
                && matches!(
                    &cmd,
                    EngineCommand::AddEffect { .. }
                        | EngineCommand::RemoveEffect(..)
                        | EngineCommand::MoveEffect { .. }
                        | EngineCommand::AddPluginEffect { .. }
                )
            {
                self.graph_edit_pending = true;
            }
            match cmd {
                EngineCommand::ArmClipRecord {
                    free_length,
                    prepared,
                    count_in_bars,
                } => self.arm_clip_record(prepared, count_in_bars, free_length),
                EngineCommand::StopClipRecord { immediate } => self.stop_clip_record(immediate),
                EngineCommand::RefreshClip(prepared) => self.refresh_clip(prepared),
                EngineCommand::EditClip { active, queued } => self.edit_clip(active, queued),
                EngineCommand::BeginClipPerformance => self.begin_clip_performance(),
                EngineCommand::QueueClips {
                    clips,
                    quantization,
                } => self.queue_clips(clips, quantization),
                EngineCommand::Play => self.command_play(),
                EngineCommand::Stop => self.command_stop(),
                EngineCommand::Seek(pos) => self.command_seek(pos),
                EngineCommand::SetBpm(bpm) => {
                    // V1 Perform holds one project tempo from the first
                    // Section transition until transport stop.
                    if !self.clip_performance
                        && self.active_section.is_none()
                        && self.pending_section_record.is_none()
                        && self.active_section_record.is_none()
                    {
                        self.transport.set_bpm(bpm);
                        self.recalculate_audio_length();
                        self.reschedule_note_repeats();
                    }
                }
                EngineCommand::SetProjectSwing(swing) => {
                    self.project_swing = swing;
                }
                EngineCommand::LaunchSection(prepared) => {
                    self.clear_clip_performance();
                    if self.pending_section_record.is_some() || self.active_section_record.is_some()
                    {
                        let event = EngineEvent::SectionQueueCancelled { retired: prepared };
                        if let Err(rtrb::PushError::Full(event)) = self.event_tx.push(event) {
                            std::mem::forget(event);
                        }
                        continue;
                    }
                    self.cancel_section_queue();
                    self.begin_performance_clock();
                    self.activate_section(prepared, self.performance_position);
                }
                EngineCommand::QueueSection {
                    prepared,
                    quantization,
                } => self.queue_section(prepared, quantization),
                EngineCommand::RefreshSection(mut prepared) => {
                    let section_id = prepared.section_id;
                    let mut applied = false;
                    if let Some(active) = self
                        .active_section
                        .as_mut()
                        .filter(|active| active.section_id == section_id)
                    {
                        let length_samples = if self.transport.bpm() > 0.0 {
                            (prepared.length_beats * self.sample_rate as f64 * 60.0
                                / self.transport.bpm())
                            .round()
                            .max(1.0) as u64
                        } else {
                            1
                        };
                        active.length_samples = length_samples;
                        active.looping = prepared.looping;
                        active.position_samples = if active.looping {
                            active.position_samples % length_samples
                        } else {
                            active.position_samples.min(length_samples)
                        };
                        for track in &mut self.tracks {
                            track.flush_notes();
                        }
                        for incoming in prepared.tracks_mut() {
                            if let Some(track) = self
                                .tracks
                                .iter_mut()
                                .find(|track| track.id == incoming.track_id)
                            {
                                std::mem::swap(
                                    &mut track.section_playback_source,
                                    &mut incoming.source,
                                );
                            }
                        }
                        applied = true;
                    }
                    if applied {
                        self.section_capture_source(
                            section_id,
                            self.effective_position(),
                            self.active_section
                                .map_or(0, |active| active.position_samples),
                            true,
                        );
                    }
                    let event = EngineEvent::SectionSourceRefreshed {
                        section_id,
                        applied,
                        effective_at_samples: self.effective_position(),
                        section_position_samples: applied.then(|| {
                            self.active_section
                                .map(|active| active.position_samples)
                                .unwrap_or(0)
                        }),
                        retired: prepared,
                    };
                    self.present_event(event, if applied { self.mix_latency() } else { 0 });
                }
                EngineCommand::ArmSectionRecord {
                    section_id,
                    track_id,
                    prepared,
                    count_in_bars,
                    replace_existing,
                } => self.arm_section_record(
                    section_id,
                    track_id,
                    prepared,
                    count_in_bars,
                    replace_existing,
                ),
                EngineCommand::StopSectionRecord => self.stop_section_record(),
                EngineCommand::LoadAudio(audio) => {
                    let len = audio.num_frames() as u64;
                    self.audio = Some(audio);
                    self.arrangement_audio_length = Some(len);
                    if !self.clip_performance
                        && self.active_section.is_none()
                        && !self.arrangement_recording
                    {
                        self.transport.set_audio_length(Some(len));
                    }
                }
                EngineCommand::UnloadAudio => {
                    let was_clip_performance = self.clip_performance;
                    self.clear_clip_performance();
                    self.stop_section_record();
                    self.stop_heard_capture();
                    self.audio = None;
                    self.arrangement_audio_length = None;
                    self.arrangement_recording = false;
                    self.active_section = None;
                    self.clock_domain = ClockDomain::Arrange;
                    self.transport.set_audio_length(None);
                    self.transport.stop();
                    if was_clip_performance {
                        self.clip_event(EngineEvent::PlaybackStopped);
                    } else {
                        let _ = self.event_tx.push(EngineEvent::PlaybackStopped);
                    }
                }
                // -- Multi-track commands --
                EngineCommand::AddTrack(id, _name) => {
                    self.tracks.push(EngineTrack::new(id));
                    self.recalculate_audio_length();
                }
                EngineCommand::RemoveTrack(id) => {
                    if let Some(pos) = self.tracks.iter().position(|t| t.id == id) {
                        let track = self.tracks.remove(pos);
                        self.dispose_channel(track);
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
                    start_marker,
                    duration,
                    loop_enabled,
                    loop_start,
                    loop_end,
                    linear_gain,
                    fades,
                    playback_direction,
                    warp_markers,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        track.playback_source.clips.push(EngineClip {
                            id: clip_id,
                            audio,
                            position,
                            source_offset,
                            start_marker,
                            duration,
                            loop_enabled,
                            loop_start,
                            loop_end,
                            linear_gain,
                            fades: fades.clamped_to(duration),
                            playback_direction,
                            warp_markers,
                        });
                    }
                    self.recalculate_audio_length();
                }
                EngineCommand::RemoveClip(track_id, clip_id) => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        track.playback_source.clips.retain(|c| c.id != clip_id);
                    }
                    self.recalculate_audio_length();
                }
                EngineCommand::ReplaceClipAudio {
                    track_id,
                    clip_id,
                    audio,
                    duration,
                    source_offset,
                    start_marker,
                    loop_start,
                    loop_end,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        if let Some(clip) = track
                            .playback_source
                            .clips
                            .iter_mut()
                            .find(|c| c.id == clip_id)
                        {
                            clip.audio = audio;
                            clip.duration = duration;
                            clip.fades = clip.fades.clamped_to(duration);
                            clip.source_offset = source_offset;
                            clip.start_marker = start_marker;
                            clip.loop_start = loop_start;
                            clip.loop_end = loop_end;
                        }
                    }
                    self.recalculate_audio_length();
                }
                EngineCommand::ReplaceClipBuffer {
                    track_id,
                    clip_id,
                    audio,
                } => {
                    if let Some(clip) = self
                        .tracks
                        .iter_mut()
                        .find(|track| track.id == track_id)
                        .and_then(|track| {
                            track
                                .playback_source
                                .clips
                                .iter_mut()
                                .find(|clip| clip.id == clip_id)
                        })
                    {
                        clip.audio = audio;
                    }
                }
                EngineCommand::MoveClip {
                    track_id,
                    clip_id,
                    new_position,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        if let Some(clip) = track
                            .playback_source
                            .clips
                            .iter_mut()
                            .find(|c| c.id == clip_id)
                        {
                            clip.position = new_position;
                        }
                    }
                    self.recalculate_audio_length();
                }
                EngineCommand::SetClipGain {
                    track_id,
                    clip_id,
                    linear_gain,
                } => {
                    if let Some(clip) = self
                        .tracks
                        .iter_mut()
                        .find(|track| track.id == track_id)
                        .and_then(|track| {
                            track
                                .playback_source
                                .clips
                                .iter_mut()
                                .find(|clip| clip.id == clip_id)
                        })
                    {
                        clip.linear_gain = linear_gain;
                    }
                }
                EngineCommand::SetClipFades {
                    track_id,
                    clip_id,
                    fades,
                } => {
                    if let Some(clip) = self
                        .tracks
                        .iter_mut()
                        .find(|track| track.id == track_id)
                        .and_then(|track| {
                            track
                                .playback_source
                                .clips
                                .iter_mut()
                                .find(|clip| clip.id == clip_id)
                        })
                    {
                        clip.fades = fades.clamped_to(clip.duration);
                    }
                }
                EngineCommand::SetClipPlaybackDirection {
                    track_id,
                    clip_id,
                    direction,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        track
                            .playback_source
                            .set_clip_playback_direction(clip_id, direction);
                    }
                }
                EngineCommand::SetClipWarpMarkers {
                    track_id,
                    clip_id,
                    warp_markers,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        track
                            .playback_source
                            .set_clip_warp_markers(clip_id, warp_markers);
                    }
                }
                EngineCommand::SetClipBounds {
                    track_id,
                    clip_id,
                    source_offset,
                    start_marker,
                    duration,
                    loop_start,
                    loop_end,
                } => {
                    if let Some(clip) = self
                        .tracks
                        .iter_mut()
                        .find(|track| track.id == track_id)
                        .and_then(|track| {
                            track
                                .playback_source
                                .clips
                                .iter_mut()
                                .find(|clip| clip.id == clip_id)
                        })
                    {
                        clip.source_offset = source_offset;
                        clip.start_marker = start_marker;
                        clip.duration = duration;
                        clip.fades = clip.fades.clamped_to(duration);
                        clip.loop_start = loop_start;
                        clip.loop_end = loop_end;
                    }
                    self.recalculate_audio_length();
                }
                EngineCommand::SetTrackGain(id, gain) => {
                    self.set_track_gain(id, gain);
                }
                EngineCommand::SetAutomationLane { track_id, lane } => {
                    if let Some(track) = self.channel_mut(track_id) {
                        match track
                            .playback_source
                            .automation
                            .iter_mut()
                            .find(|l| l.id == lane.id)
                        {
                            Some(existing) => *existing = lane,
                            None => track.playback_source.automation.push(lane),
                        }
                    }
                }
                EngineCommand::RemoveAutomationLane { track_id, lane_id } => {
                    if let Some(track) = self.channel_mut(track_id) {
                        track.playback_source.automation.retain(|l| l.id != lane_id);
                    }
                }
                EngineCommand::SetTrackPan(id, pan) => {
                    self.set_track_pan(id, pan);
                }
                EngineCommand::SetTrackMute(id, mute) => {
                    self.set_track_mute(id, mute);
                }
                EngineCommand::QueueTrackMute {
                    track_id,
                    muted,
                    quantization,
                } => {
                    self.queue_track_mute(track_id, muted, quantization);
                }
                EngineCommand::SetAutomationOverride {
                    track_id,
                    target,
                    overridden,
                } => {
                    self.set_automation_override(track_id, target, overridden);
                }
                EngineCommand::UpdateAutomationGesture {
                    track_id,
                    target,
                    normalized_value,
                    begin,
                } => {
                    self.update_automation_gesture(track_id, target, normalized_value, begin);
                }
                EngineCommand::EndAutomationGesture { track_id, target } => {
                    self.end_automation_gesture(track_id, target);
                }

                // -- Busses --
                EngineCommand::AddBus(id, _name) => {
                    self.buses.push(EngineTrack::new(id));
                }
                EngineCommand::RemoveBus(id) => {
                    if let Some(pos) = self.buses.iter().position(|b| b.id == id) {
                        let bus = self.buses.remove(pos);
                        self.dispose_channel(bus);
                    }
                    self.pending_bus_cleanup = Some((id, 0));
                    self.clean_removed_bus_automation();
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
                EngineCommand::SetTrackSwingOffset(id, offset) => {
                    self.set_track_swing_offset(id, offset);
                }

                // -- Infrastructure --
                EngineCommand::SetSampleRate(sr) => {
                    self.sample_rate = sr;
                    self.recalculate_audio_length();
                    self.reschedule_note_repeats();
                }
                EngineCommand::SetSpectrumTap(target) => {
                    self.spectrum_track = target;
                }

                // -- Arrangement recording / looping --
                EngineCommand::SetArrangementRecording(active) => {
                    self.arrangement_recording = active;
                    if self.active_section.is_none() {
                        self.transport.set_audio_length(if active {
                            None
                        } else {
                            self.arrangement_audio_length
                        });
                    }
                }
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
                        if let Some(clip) = track
                            .playback_source
                            .clips
                            .iter_mut()
                            .find(|c| c.id == clip_id)
                        {
                            clip.loop_enabled = enabled;
                            clip.loop_start = loop_start;
                            clip.loop_end = loop_end;
                        }
                    }
                }
                EngineCommand::SetClipStartMarker {
                    track_id,
                    clip_id,
                    start_marker,
                } => {
                    if let Some(clip) = self
                        .tracks
                        .iter_mut()
                        .find(|track| track.id == track_id)
                        .and_then(|track| {
                            track
                                .playback_source
                                .clips
                                .iter_mut()
                                .find(|clip| clip.id == clip_id)
                        })
                    {
                        clip.start_marker = start_marker;
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
                        if let Some(clip) = track
                            .playback_source
                            .note_clips
                            .iter_mut()
                            .find(|c| c.id == clip_id)
                        {
                            clip.loop_enabled = enabled;
                            clip.loop_start_beats = loop_start_beats;
                            clip.loop_end_beats = loop_end_beats;
                        }
                        track.flush_notes();
                    }
                }
                EngineCommand::SetNoteClipStartMarker {
                    track_id,
                    clip_id,
                    start_marker_beats,
                } => {
                    if let Some(track) = self.tracks.iter_mut().find(|track| track.id == track_id) {
                        if let Some(clip) = track
                            .playback_source
                            .note_clips
                            .iter_mut()
                            .find(|clip| clip.id == clip_id)
                        {
                            clip.start_marker_beats = start_marker_beats;
                            track.flush_notes();
                        }
                    }
                }

                // -- Dedicated Audition Bus --
                EngineCommand::StartAudition {
                    audio,
                    start,
                    looped,
                } => {
                    let fade_frames = audition_fade_frames(self.sample_rate);
                    if self.transport.is_playing() && start == AuditionStart::NextBar {
                        let clock_position = self.effective_position();
                        let target = next_audition_boundary(
                            clock_position,
                            self.transport.bpm(),
                            self.sample_rate,
                            4,
                        );
                        let frames_until_start = target.saturating_sub(clock_position);
                        self.audition
                            .queue(audio, frames_until_start, fade_frames, looped, true);
                        let _ = self.event_tx.push(EngineEvent::AuditionQueued);
                    } else {
                        self.audition.start(
                            audio,
                            fade_frames,
                            looped,
                            start == AuditionStart::NextBar,
                        );
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

                // -- External MIDI input --
                EngineCommand::StartNoteRepeat {
                    id,
                    track_id,
                    pitch,
                    velocity,
                    rate,
                } => {
                    let position = self.performance_position;
                    let bpm = self.transport.bpm();
                    let sample_rate = self.sample_rate;
                    let project_swing = self.project_swing;
                    let Some(track_index) =
                        self.tracks.iter().position(|track| track.id == track_id)
                    else {
                        continue;
                    };
                    let playing = self.transport.is_playing();
                    let had_active_repeats = self.has_active_note_repeats();
                    let anchor_sample = if playing {
                        self.playing_note_repeat_anchor()
                    } else {
                        *self.stopped_note_repeat_anchor.get_or_insert(position)
                    };
                    let sound_immediately = !playing && !had_active_repeats;
                    if sound_immediately {
                        if let Some(instrument) = self.tracks[track_index].instrument.as_mut() {
                            instrument.note_on(pitch, velocity);
                        }
                        let recording = self.source_recording_position();
                        let event = EngineEvent::NoteRepeated {
                            track_id,
                            pitch,
                            velocity,
                            rate,
                            effective_at_samples: position,
                            canonical_at_samples: position,
                            section_id: self.active_section.map(|active| active.section_id),
                            section_position_samples: self
                                .active_section
                                .map(|active| active.position_samples),
                            canonical_section_position_samples: self
                                .active_section
                                .map(|active| active.position_samples),
                        };
                        let source = EngineEvent::SourceNoteRepeated {
                            track_id,
                            pitch,
                            velocity,
                            rate,
                            position: recording,
                        };
                        if !presentation::emit_repeated(
                            source,
                            event,
                            &mut self.event_tx,
                            &mut self.scheduled_presentation,
                            self.output_position,
                            self.output_position,
                        ) {
                            self.fail_presentation();
                        }
                    }
                    self.tracks[track_index].start_note_repeat(
                        NoteRepeatStart {
                            id,
                            pitch,
                            velocity,
                            rate,
                        },
                        NoteRepeatClock {
                            after_sample: position,
                            anchor_sample,
                            include_after_sample: !sound_immediately,
                            bpm,
                            sample_rate,
                            swing: project_swing,
                        },
                    );
                }
                EngineCommand::UpdateNoteRepeatRate { id, track_id, rate } => {
                    let position = self.performance_position;
                    let bpm = self.transport.bpm();
                    let sample_rate = self.sample_rate;
                    let project_swing = self.project_swing;
                    if let Some(track) = self.tracks.iter_mut().find(|track| track.id == track_id) {
                        track.update_note_repeat_rate(
                            id,
                            rate,
                            position,
                            bpm,
                            sample_rate,
                            project_swing,
                        );
                    }
                }
                EngineCommand::StopNoteRepeat { id, track_id } => {
                    if let Some(track) = self.tracks.iter_mut().find(|track| track.id == track_id) {
                        track.stop_note_repeat(id);
                    }
                    if !self.transport.is_playing() && !self.has_active_note_repeats() {
                        self.stopped_note_repeat_anchor = None;
                    }
                }

                EngineCommand::AddPluginEffect {
                    track_id,
                    effect_id,
                    effect,
                    position,
                } => self.command_add_plugin_effect(track_id, effect_id, effect, position),
                EngineCommand::AuditionNote {
                    track_id,
                    pitch,
                    velocity,
                    on,
                } => self.command_audition_note(track_id, pitch, velocity, on),
                EngineCommand::SetPluginInstrument {
                    track_id,
                    instrument,
                } => self.command_set_plugin_instrument(track_id, instrument),
                EngineCommand::UpdateAutomationRouting(prepared) => {
                    self.command_update_automation_routing(prepared)
                }
                EngineCommand::RejectRoutingUpdate { reason } => {
                    self.command_reject_routing_update(reason)
                }
                EngineCommand::ResumeDeviceReconfiguration { device, routing } => {
                    self.command_resume_device_reconfiguration(device, routing)
                }
                EngineCommand::RejectDeviceReconfiguration { device, reason } => {
                    self.command_reject_device_reconfiguration(device, reason)
                }
                EngineCommand::SetRouting(prepared) => self.command_set_routing(prepared),
                EngineCommand::StartPerformanceCapture => self.command_start_performance_capture(),
                EngineCommand::StopPerformanceCapture => self.command_stop_performance_capture(),
                EngineCommand::ExternalNoteOn {
                    track_id,
                    pitch,
                    velocity,
                } => self.command_external_note_on(track_id, pitch, velocity),
                EngineCommand::ExternalNoteOff { track_id, pitch } => {
                    self.command_external_note_off(track_id, pitch)
                }
                EngineCommand::AddEffect {
                    track_id,
                    effect_id,
                    effect_type,
                    position,
                } => self.command_add_effect(track_id, effect_id, effect_type, position),
                EngineCommand::RemoveEffect(track_id, effect_id) => {
                    self.command_remove_effect(track_id, effect_id)
                }
                EngineCommand::SetEffectParam {
                    track_id,
                    effect_id,
                    param_index,
                    value,
                } => self.command_set_effect_param(track_id, effect_id, param_index, value),
                EngineCommand::SetEffectBypass {
                    track_id,
                    effect_id,
                    bypass,
                } => self.command_set_effect_bypass(track_id, effect_id, bypass),
                EngineCommand::MoveEffect {
                    track_id,
                    effect_id,
                    new_index,
                } => self.command_move_effect(track_id, effect_id, new_index),
                EngineCommand::AddInstrumentTrack(id, _name, kind) => {
                    self.command_add_instrument_track(id, _name, kind)
                }
                EngineCommand::AddMidiTrack(id, _name) => self.command_add_midi_track(id, _name),
                EngineCommand::SetTrackInstrument(track_id, kind) => {
                    self.command_set_track_instrument(track_id, kind)
                }
                EngineCommand::RemoveTrackInstrument(track_id) => {
                    self.command_remove_track_instrument(track_id)
                }
                EngineCommand::SetNoteClipDuration {
                    track_id,
                    clip_id,
                    duration_beats,
                } => self.command_set_note_clip_duration(track_id, clip_id, duration_beats),
                EngineCommand::SetNoteClipGrooveGrid {
                    track_id,
                    clip_id,
                    groove_grid,
                } => self.command_set_note_clip_groove_grid(track_id, clip_id, groove_grid),
                EngineCommand::AddNoteClip {
                    track_id,
                    clip_id,
                    position_beats,
                    duration_beats,
                    start_marker_beats,
                    loop_enabled,
                    loop_start_beats,
                    loop_end_beats,
                    groove_grid,
                } => self.command_add_note_clip(
                    track_id,
                    clip_id,
                    position_beats,
                    duration_beats,
                    start_marker_beats,
                    loop_enabled,
                    loop_start_beats,
                    loop_end_beats,
                    groove_grid,
                ),
                EngineCommand::RemoveNoteClip(track_id, clip_id) => {
                    self.command_remove_note_clip(track_id, clip_id)
                }
                EngineCommand::MoveNoteClip {
                    track_id,
                    clip_id,
                    new_position_beats,
                } => self.command_move_note_clip(track_id, clip_id, new_position_beats),
                EngineCommand::AddNote {
                    track_id,
                    clip_id,
                    note,
                } => self.command_add_note(track_id, clip_id, note),
                EngineCommand::RemoveNote {
                    track_id,
                    clip_id,
                    note_index,
                } => self.command_remove_note(track_id, clip_id, note_index),
                EngineCommand::EditNote {
                    track_id,
                    clip_id,
                    note_index,
                    note,
                } => self.command_edit_note(track_id, clip_id, note_index, note),
                EngineCommand::SetInstrumentParam {
                    track_id,
                    param_index,
                    value,
                } => self.command_set_instrument_param(track_id, param_index, value),
                EngineCommand::LoadSamplerSample {
                    track_id,
                    sample,
                    sample_name,
                } => self.command_load_sampler_sample(track_id, sample, sample_name),
                EngineCommand::LoadDrumRackPadSample {
                    track_id,
                    pad_index,
                    sample,
                    sample_name,
                } => {
                    self.command_load_drum_rack_pad_sample(track_id, pad_index, sample, sample_name)
                }
                EngineCommand::ClearDrumRackPad {
                    track_id,
                    pad_index,
                } => self.command_clear_drum_rack_pad(track_id, pad_index),
                EngineCommand::SetDrumRackPadState {
                    track_id,
                    pad_index,
                    state,
                } => self.command_set_drum_rack_pad_state(track_id, pad_index, state),
            }
        }
    }

    pub(super) fn recalculate_audio_length(&mut self) {
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
        if !self.clip_performance && self.active_section.is_none() && !self.arrangement_recording {
            self.transport
                .set_audio_length(self.arrangement_audio_length);
        }
    }
}
