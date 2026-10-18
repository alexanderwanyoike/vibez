//! Audio-thread engine command dispatch (`AudioEngine::drain_commands`).
//! Split from engine.rs. Runs in the audio callback: keep allocation-free and lock-free.

use super::*;
use crate::events::SectionSourceRefreshedEvent;
use vibez_core::id::ClipId;

impl AudioEngine {
    /// Drain all pending commands from the ring buffer without blocking.
    pub(super) fn drain_commands(&mut self) {
        self.rendered_callback_frames = 0;
        self.flush_presentation();
        self.flush_section_record_stop();
        self.flush_returning_section();
        if self.pending_source_cleanup {
            self.pending_source_cleanup = false;
            self.continue_clear_clip_sources();
            self.cancel_queued_track_mutes();
            if self.pending_source_cleanup {
                return;
            }
        }
        self.continue_capture_replay();
        if self.capture_replay.is_some() {
            return;
        }
        self.continue_clip_batch(self.performance_position);
        if self.clip_batch_blocks_commands() {
            return;
        }
        self.return_retired_routing();
        self.flush_retirements();
        self.clean_removed_bus_automation();
        self.continue_clip_insertion();
        loop {
            self.flush_retirements();
            if !self.flush_removed_clip_owners() {
                break;
            }
            if self.pending_clip_insertion.is_some()
                || self.capture_replay.is_some()
                || self.clip_batch_blocks_commands()
            {
                break;
            }
            let Ok(_) = self.cmd_rx.peek() else {
                break;
            };
            // Track-sized cancellation retains the remaining owners/intents.
            // Admission covers only the bounded command body.
            if self.scheduled_presentation.capacity() - self.scheduled_presentation.len()
                < presentation_queue::COMMAND_PRESENTATION_RESERVE
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
                || (self.pending_section_record_stop.is_some()
                    && matches!(
                        self.cmd_rx.peek(),
                        Ok(EngineCommand::ArmSectionRecord { .. })
                    ))
                || (self
                    .queued_section
                    .as_ref()
                    .is_some_and(|queued| queued.phase == SectionQueuePhase::Returning)
                    && matches!(
                        self.cmd_rx.peek(),
                        Ok(EngineCommand::LaunchSection(_)
                            | EngineCommand::QueueSection { .. }
                            | EngineCommand::ArmSectionRecord { .. })
                    ))
                || self.pending_retirements.available()
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
            let budget = self.cmd_rx.peek().map_or(2, |cmd| cmd.retirement_reserve());
            let Some(mut allowance) = self.pending_retirements.reserve(budget) else {
                break;
            };
            let Ok(cmd) = self.cmd_rx.pop() else {
                break;
            };
            let bus_cleanup = self.dispatch_command(cmd, &mut allowance);
            // The command's conservative unused credits cannot block the
            // independently admitted, track-sized lane cleanup at this point.
            drop(allowance);
            if bus_cleanup {
                self.clean_removed_bus_automation();
            }
        }
    }

    fn arrangement_clip_mut(
        &mut self,
        track_id: TrackId,
        clip_id: ClipId,
    ) -> Option<&mut EngineClip> {
        self.tracks
            .iter_mut()
            .find(|track| track.id == track_id)
            .and_then(|track| {
                track
                    .playback_source
                    .clips
                    .iter_mut()
                    .find(|clip| clip.id == clip_id)
            })
    }

    fn dispatch_command(
        &mut self,
        mut cmd: EngineCommand,
        allowance: &mut RetirementAllowance,
    ) -> bool {
        self.prepare_device_edit(&mut cmd);
        if self.retain_device_parameter(&cmd) {
            return false;
        }
        if cmd.edits_effect_chain() {
            self.compensation_plan.expect_structural_plan();
        }
        let mut bus_cleanup = false;
        match cmd {
            EngineCommand::SetRouting(prepared) => self.command_set_routing(prepared),
            EngineCommand::UpdateAutomationRouting(prepared) => {
                self.command_update_automation_routing(prepared)
            }
            EngineCommand::RejectRoutingUpdate { reason } => {
                self.command_reject_routing_update(reason, allowance)
            }
            EngineCommand::ResumeDeviceReconfiguration { device, routing } => {
                self.command_resume_device_reconfiguration(device, routing, allowance)
            }
            EngineCommand::RejectDeviceReconfiguration { device, reason } => {
                self.command_reject_device_reconfiguration(device, reason, allowance)
            }
            EngineCommand::StartPerformanceCapture => {
                self.command_start_performance_capture(allowance)
            }
            EngineCommand::StopPerformanceCapture => {
                self.command_stop_performance_capture(allowance)
            }
            EngineCommand::ExternalNoteOn {
                track_id,
                pitch,
                velocity,
            } => self.command_external_note_on(track_id, pitch, velocity, allowance),
            EngineCommand::ExternalNoteOff { track_id, pitch } => {
                self.command_external_note_off(track_id, pitch, allowance)
            }
            EngineCommand::ArmClipRecord {
                free_length,
                prepared,
                count_in_bars,
            } => self.arm_clip_record(prepared, count_in_bars, free_length, allowance),
            EngineCommand::StopClipRecord { immediate } => {
                self.stop_clip_record(immediate, allowance)
            }
            EngineCommand::RefreshClip(prepared) => self.refresh_clip(prepared, allowance),
            EngineCommand::EditClip { active, queued } => self.edit_clip(active, queued, allowance),
            EngineCommand::BeginClipPerformance => self.begin_clip_performance(allowance),
            EngineCommand::QueueClips {
                clips,
                quantization,
            } => self.queue_clips(clips, quantization, allowance),
            EngineCommand::Play => self.command_play(allowance),
            EngineCommand::Stop => self.command_stop(allowance),
            EngineCommand::Seek(pos) => self.command_seek(pos, allowance),
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
                self.clear_clip_performance(allowance);
                if self.pending_section_record.is_some() || self.active_section_record.is_some() {
                    let event = EngineEvent::SectionQueueCancelled { retired: prepared };
                    self.present_event(event, allowance);
                    return false;
                }
                self.cancel_section_queue();
                self.begin_performance_clock(allowance);
                self.activate_section(prepared, self.performance_position);
            }
            EngineCommand::QueueSection {
                prepared,
                quantization,
            } => self.queue_section(prepared, quantization, allowance),
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
                        allowance,
                    );
                }
                let event = EngineEvent::SectionSourceRefreshed(SectionSourceRefreshedEvent {
                    section_id,
                    applied,
                    effective_at_samples: self.effective_position(),
                    section_position_samples: applied.then(|| {
                        self.active_section
                            .map(|active| active.position_samples)
                            .unwrap_or(0)
                    }),
                    retired: crate::events::DisposalCell::new(prepared),
                });
                self.present_event(event, allowance);
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
                allowance,
            ),
            EngineCommand::StopSectionRecord => self.stop_section_record(),
            EngineCommand::UnloadAudio => {
                self.cancel_presentation();
                let was_clip_performance = self.clip_performance;
                self.clear_clip_performance(allowance);
                self.stop_section_record();
                self.stop_heard_capture(allowance);
                self.arrangement_audio_length = None;
                self.arrangement_recording = false;
                self.active_section = None;
                self.clock_domain = ClockDomain::Arrange;
                self.transport.set_audio_length(None);
                self.transport.stop();
                if was_clip_performance {
                    self.clip_event(EngineEvent::PlaybackStopped, allowance);
                } else {
                    self.present_event(EngineEvent::PlaybackStopped, allowance);
                }
            }
            // -- Multi-track commands --
            EngineCommand::AddTrack(track) => {
                if let Some(track) = admit_channel(&mut self.tracks, track) {
                    self.dispose_channel(track, allowance);
                }
                self.recalculate_audio_length();
            }
            EngineCommand::RemoveTrack(id) => {
                if let Some(pos) = self.tracks.iter().position(|t| t.id == id) {
                    let track = self.tracks.remove(pos);
                    self.dispose_channel(track, allowance);
                }
                self.recalculate_audio_length();
            }
            EngineCommand::ReorderTracks(order) => {
                let mut destination = 0;
                for id in &order {
                    while let Some(offset) = self.tracks[destination..]
                        .iter()
                        .position(|track| track.id == *id)
                    {
                        // A stable prefix move keeps unspecified channels in
                        // order without the stable sort's scratch allocation.
                        self.tracks[destination..=destination + offset].rotate_right(1);
                        destination += 1;
                    }
                }
                self.retire_event(EngineEvent::RetiredTrackOrder(order), allowance);
            }
            EngineCommand::AddClip {
                insertion,
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
                self.command_add_clip(
                    track_id,
                    EngineClip {
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
                    },
                    insertion,
                    allowance,
                );
            }
            EngineCommand::RemoveClip(track_id, clip_id) => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    track.playback_source.remove_audio_clips(clip_id);
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
                let retired = if let Some(clip) = self.arrangement_clip_mut(track_id, clip_id) {
                    let retired = std::mem::replace(&mut clip.audio, audio);
                    clip.duration = duration;
                    clip.fades = clip.fades.clamped_to(duration);
                    clip.source_offset = source_offset;
                    clip.start_marker = start_marker;
                    clip.loop_start = loop_start;
                    clip.loop_end = loop_end;
                    retired
                } else {
                    audio
                };
                self.retire_event(EngineEvent::RetiredAudioBuffer(retired), allowance);
                self.recalculate_audio_length();
            }
            EngineCommand::ReplaceClipBuffer {
                track_id,
                clip_id,
                audio,
            } => {
                let retired = if let Some(clip) = self.arrangement_clip_mut(track_id, clip_id) {
                    std::mem::replace(&mut clip.audio, audio)
                } else {
                    audio
                };
                self.retire_event(EngineEvent::RetiredAudioBuffer(retired), allowance);
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
                let retired = if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id)
                {
                    track
                        .playback_source
                        .set_clip_warp_markers(clip_id, warp_markers)
                } else {
                    warp_markers
                };
                self.retire_event(EngineEvent::RetiredWarpMarkers(retired), allowance);
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
            EngineCommand::SetAutomationLane {
                track_id,
                insertion,
            } => self.command_set_automation_lane(track_id, insertion, allowance),
            EngineCommand::RemoveAutomationLane { track_id, lane_id } => {
                self.remove_automation_lane(track_id, lane_id, allowance)
            }
            EngineCommand::SetTrackPan(id, pan) => {
                self.set_track_pan(id, pan);
            }
            EngineCommand::SetTrackMute(id, mute) => {
                self.set_track_mute(id, mute, allowance);
            }
            EngineCommand::QueueTrackMute {
                track_id,
                muted,
                quantization,
            } => {
                self.queue_track_mute(track_id, muted, quantization, allowance);
            }
            EngineCommand::SetAutomationOverride {
                track_id,
                target,
                overridden,
            } => {
                self.set_automation_override(track_id, target, overridden, allowance);
            }
            EngineCommand::UpdateAutomationGesture {
                track_id,
                target,
                normalized_value,
                begin,
            } => {
                self.update_automation_gesture(
                    track_id,
                    target,
                    normalized_value,
                    begin,
                    allowance,
                );
            }
            EngineCommand::EndAutomationGesture { track_id, target } => {
                self.end_automation_gesture(track_id, target, allowance);
            }

            // -- Busses --
            EngineCommand::AddBus(bus) => {
                if let Some(bus) = admit_channel(&mut self.buses, bus) {
                    self.dispose_channel(bus, allowance);
                }
            }
            EngineCommand::RemoveBus(id) => {
                if let Some(pos) = self.buses.iter().position(|b| b.id == id) {
                    let bus = self.buses.remove(pos);
                    self.dispose_channel(bus, allowance);
                }
                self.pending_bus_cleanup = Some((id, 0));
                bus_cleanup = true;
            }
            EngineCommand::SetSend {
                track_id,
                bus_id,
                amount,
            } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    track.set_send(bus_id, amount.clamp(0.0, 1.0));
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
            EngineCommand::SetEffectSpectrumTap(target) => {
                self.effect_spectrum.select(target);
            }
            EngineCommand::SetEffectMeterChannel {
                track_id,
                generation,
            } => self.set_effect_meter_channel(track_id, generation),

            // -- Arrangement recording / looping --
            EngineCommand::RemoveEffect(track_id, effect_id) => {
                self.command_remove_effect(track_id, effect_id, allowance)
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
            EngineCommand::RemoveTrackInstrument(track_id) => {
                self.command_remove_track_instrument(track_id, allowance)
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
                insertion,
            } => self.command_add_note_clip(track_id, insertion, allowance),
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
                insertion,
            } => self.command_add_note(track_id, clip_id, insertion, allowance),
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
            } => self.command_load_sampler_sample(track_id, sample, sample_name, allowance),
            EngineCommand::LoadDrumRackPadSample {
                track_id,
                pad_index,
                sample,
                sample_name,
            } => self.command_load_drum_rack_pad_sample(
                track_id,
                pad_index,
                sample,
                sample_name,
                allowance,
            ),
            EngineCommand::ClearDrumRackPad {
                track_id,
                pad_index,
            } => self.command_clear_drum_rack_pad(track_id, pad_index, allowance),
            EngineCommand::SetDrumRackPadState {
                track_id,
                pad_index,
                state,
            } => self.command_set_drum_rack_pad_state(track_id, pad_index, state, allowance),
            EngineCommand::SetDrumRackPadParam {
                track_id,
                pad_index,
                param_index,
                value,
            } => self.command_set_drum_rack_pad_param(track_id, pad_index, param_index, value),
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
                    self.present_event(EngineEvent::AuditionQueued, allowance);
                } else {
                    self.audition.start(
                        audio,
                        fade_frames,
                        looped,
                        start == AuditionStart::NextBar,
                    );
                    self.present_event(EngineEvent::AuditionStarted, allowance);
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
                    self.present_event(EngineEvent::AuditionStopped, allowance);
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
                let Some(track_index) = self.tracks.iter().position(|track| track.id == track_id)
                else {
                    return false;
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
                    let trigger = crate::note_repeat::NoteRepeatTrigger {
                        pitch,
                        velocity,
                        rate,
                        effective_at_samples: position,
                        canonical_at_samples: position,
                    };
                    let (source, event) =
                        EngineEvent::repeated_pair(track_id, trigger, recording, recording.into());
                    if !presentation_queue::emit_repeated(
                        source,
                        event,
                        &mut self.event_tx,
                        &mut self.scheduled_presentation,
                        self.output_position,
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

            EngineCommand::InsertEffect {
                track_id,
                effect_id,
                effect,
                position,
            } => self.command_insert_effect(track_id, effect_id, effect, position, allowance),
            EngineCommand::AuditionNote {
                track_id,
                pitch,
                velocity,
                on,
            } => self.command_audition_note(track_id, pitch, velocity, on),
            EngineCommand::SetInstrument {
                track_id,
                instrument,
            } => self.command_set_instrument(track_id, instrument, allowance),
        }
        bus_cleanup
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
        self.arrangement_audio_length = (total > 0).then_some(total);
        if !self.clip_performance && self.active_section.is_none() && !self.arrangement_recording {
            self.transport
                .set_audio_length(self.arrangement_audio_length);
        }
    }
}

/// Returns the channel when its reserved list is full, so it is retired
/// instead of growing the list in the callback.
fn admit_channel(
    channels: &mut crate::mixer::ChannelList,
    channel: Box<EngineTrack>,
) -> Option<Box<EngineTrack>> {
    if channels.len() < channels.capacity() {
        channels.push(channel);
        None
    } else {
        debug_assert!(false, "the UI keeps channels within CHANNEL_CAPACITY");
        Some(channel)
    }
}
