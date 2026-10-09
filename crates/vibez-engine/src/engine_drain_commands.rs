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
                self.compensation_valid = false;
                self.transport.stop();
                let _ = self.event_tx.push(EngineEvent::CompensationInvalid {
                    track_id: TrackId::MASTER,
                    effect_id: None,
                    reason: "Audio presentation event history is full",
                });
                break;
            }
            if self.scheduled_presentation.len() + 2 >= self.scheduled_presentation.capacity()
                || self.section_capture_timing.len() + 2 >= self.section_capture_timing.capacity()
                || self.pending_bus_cleanup.is_some()
                || self.pending_retirements.len() == self.pending_retirements.capacity()
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
            let Ok(cmd) = self.cmd_rx.pop() else {
                break;
            };
            if self.routing.is_some()
                && matches!(
                    &cmd,
                    EngineCommand::AddEffect { .. }
                        | EngineCommand::RemoveEffect(..)
                        | EngineCommand::MoveEffect { .. }
                        | EngineCommand::AddPluginEffect { .. }
                        | EngineCommand::SetPluginInstrument { .. }
                        | EngineCommand::SetTrackInstrument(..)
                        | EngineCommand::RemoveTrackInstrument(..)
                )
            {
                self.graph_edit_pending = true;
            }
            let cmd = match self.handle_device_command(cmd) {
                Ok(()) => continue,
                Err(cmd) => cmd,
            };
            let cmd = match self.handle_compensation_command(cmd) {
                Ok(()) => continue,
                Err(cmd) => cmd,
            };
            let cmd = match self.handle_capture_command(cmd) {
                Ok(()) => continue,
                Err(cmd) => cmd,
            };
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
                EngineCommand::Play => {
                    if !self.transport.is_playing() {
                        for track in self
                            .tracks
                            .iter_mut()
                            .chain(self.buses.iter_mut())
                            .chain(std::iter::once(&mut self.master))
                        {
                            for slot in &mut track.effects {
                                slot.effect.reset();
                            }
                        }
                    }
                    if let Some(routing) = self.routing.as_mut() {
                        self.compensation_transition_frames = 0;
                        routing.clear_history();
                    }
                    let was_clip_performance = self.clip_performance;
                    self.clear_clip_performance();
                    self.clock_domain = ClockDomain::Arrange;
                    if !self.arrangement_recording {
                        self.transport
                            .set_audio_length(self.arrangement_audio_length);
                    }
                    self.transport.play();
                    let audition_queued = self.audition.resync_on_transport_start(
                        self.transport.position(),
                        self.transport.bpm(),
                        self.sample_rate,
                        audition_fade_frames(self.sample_rate),
                    );
                    if let Some(queued) = audition_queued {
                        let event = if queued {
                            EngineEvent::AuditionQueued
                        } else {
                            EngineEvent::AuditionStarted
                        };
                        let _ = self.event_tx.push(event);
                    }
                    self.performance_position = self.transport.position();
                    self.stopped_note_repeat_anchor = None;
                    let anchor = self.playing_note_repeat_anchor();
                    self.reanchor_note_repeats(anchor, self.performance_position);
                    if was_clip_performance {
                        self.clip_event(EngineEvent::PlaybackStarted);
                    } else {
                        let _ = self.event_tx.push(EngineEvent::PlaybackStarted);
                    }
                }
                EngineCommand::Stop => {
                    let was_clip_performance = self.clip_performance;
                    self.stop_section_record();
                    self.stop_heard_capture();
                    self.clear_clip_performance();
                    self.transport.stop();
                    self.arrangement_recording = false;
                    self.clock_domain = ClockDomain::Arrange;
                    self.performance_position = self.transport.position();
                    self.cancel_section_queue();
                    self.cancel_queued_track_mutes();
                    self.active_section = None;
                    self.transport
                        .set_audio_length(self.arrangement_audio_length);
                    for track in &mut self.tracks {
                        track.flush_notes();
                    }
                    self.stopped_note_repeat_anchor = self
                        .has_active_note_repeats()
                        .then_some(self.performance_position);
                    if let Some(anchor) = self.stopped_note_repeat_anchor {
                        self.reanchor_note_repeats(anchor, self.performance_position);
                    }
                    if was_clip_performance {
                        self.clip_event(EngineEvent::PlaybackStopped);
                    } else {
                        let _ = self.event_tx.push(EngineEvent::PlaybackStopped);
                    }
                }
                EngineCommand::Seek(pos) => {
                    if self.clock_domain == ClockDomain::Arrange {
                        self.stop_heard_capture();
                        if let Some(routing) = self.routing.as_mut() {
                            self.compensation_transition_frames = 0;
                            routing.clear_history();
                        }
                        for track in self
                            .tracks
                            .iter_mut()
                            .chain(self.buses.iter_mut())
                            .chain(std::iter::once(&mut self.master))
                        {
                            for slot in &mut track.effects {
                                slot.effect.reset();
                            }
                            if let Some(instrument) = track.instrument.as_mut() {
                                instrument.reset();
                            }
                        }
                    }
                    self.transport.seek(pos);
                    for track in &mut self.tracks {
                        track.flush_notes();
                    }
                    if self.clock_domain == ClockDomain::Arrange {
                        self.performance_position = pos;
                        self.reschedule_note_repeats();
                    }
                }
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

                command @ (EngineCommand::AddEffect { .. }
                | EngineCommand::RemoveEffect(..)
                | EngineCommand::SetEffectParam { .. }
                | EngineCommand::SetEffectBypass { .. }
                | EngineCommand::MoveEffect { .. }) => self.apply_effect_command(command),

                command @ (EngineCommand::AddInstrumentTrack(..)
                | EngineCommand::AddMidiTrack(..)
                | EngineCommand::SetTrackInstrument(..)
                | EngineCommand::RemoveTrackInstrument(..)
                | EngineCommand::SetNoteClipDuration { .. }
                | EngineCommand::SetNoteClipGrooveGrid { .. }
                | EngineCommand::AddNoteClip { .. }
                | EngineCommand::RemoveNoteClip { .. }
                | EngineCommand::MoveNoteClip { .. }
                | EngineCommand::AddNote { .. }
                | EngineCommand::RemoveNote { .. }
                | EngineCommand::EditNote { .. }
                | EngineCommand::SetInstrumentParam { .. }
                | EngineCommand::LoadSamplerSample { .. }
                | EngineCommand::LoadDrumRackPadSample { .. }
                | EngineCommand::ClearDrumRackPad { .. }
                | EngineCommand::SetDrumRackPadState { .. }) => {
                    self.apply_instrument_command(command)
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
                        let _ = self.event_tx.push(EngineEvent::NoteRepeated {
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
                        });
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

                _ => unreachable!("device commands are handled before transport commands"),
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
