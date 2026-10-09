//! UI-thread consumption of audio-engine events.

use std::sync::Arc;

use vibez_engine::events::EngineEvent;

use crate::domains::perform::CapturedTimelineSource;
use crate::state::AuditionMode;

use super::*;

fn apply_track_mute_event(
    state: &mut crate::state::AppState,
    track_id: vibez_core::id::TrackId,
    muted: bool,
) -> bool {
    let pending = state.perform.pending_track_mute(track_id).is_some();
    if pending && state.project_tracks.find(track_id).is_some() {
        let snapshot = state.project_snapshot();
        state.project.history.push_edit(snapshot, None);
        state.project.dirty = true;
    }
    state.perform.take_pending_track_mute(track_id);
    if let Some(track) = state.find_track_mut(track_id) {
        track.mute = muted;
    }
    pending
}

fn active_audition_status(status: &str) -> bool {
    matches!(
        status,
        RAW_AUDITION_PLAYING | WARP_AUDITION_PLAYING | WARP_AUDITION_PREPARING
    )
}

fn apply_drum_pad_flash(
    view: &mut crate::state::ViewState,
    event: &EngineEvent,
    now: std::time::Instant,
) {
    match event {
        EngineEvent::TrackNoteActivity {
            track_id,
            triggered_notes,
        } => {
            let mut notes = *triggered_notes;
            while notes != 0 {
                let pitch = notes.trailing_zeros() as u8;
                view.trigger_drum_pad_flash(*track_id, pitch, now);
                notes &= notes - 1;
            }
        }
        EngineEvent::NoteRepeated {
            track_id, pitch, ..
        }
        | EngineEvent::InstrumentNoteInput {
            track_id,
            pitch,
            on: true,
            ..
        } => view.trigger_drum_pad_flash(*track_id, *pitch, now),
        _ => {}
    }
}

fn apply_clip_resync(
    state: &mut crate::state::AppState,
    snapshot: vibez_engine::events::ClipTrackState,
) {
    state
        .perform
        .clip_editor
        .resync_track(snapshot, &state.perform.clips);

    if state.perform.clip_editor.next_request <= snapshot.through_request {
        state.transport.playing = snapshot.transport_playing;
    }
    let source = state
        .perform
        .clip_editor
        .playing
        .get(&snapshot.track_id)
        .map(|clip| CapturedTimelineSource::from_clip(clip, state.transport.samples_per_beat()));
    state.perform.capture.clip_transition(
        snapshot.track_id,
        source,
        snapshot.effective_at_samples,
        snapshot.playing.map_or(0, |active| active.position),
    );
}

impl App {
    pub(super) fn poll_engine_events(&mut self) {
        if let Some(command) = self
            .state
            .perform
            .clip_record
            .arm_audio_when_ready(self.input_bridge.record_start_position())
        {
            self.send_command(command);
        }
        let mut completed_section_recordings = Vec::new();
        let mut completed_captures = Vec::new();
        {
            while let Some(event) = self.event_rx.as_mut().and_then(|rx| rx.pop().ok()) {
                apply_drum_pad_flash(&mut self.state.view, &event, std::time::Instant::now());
                match event {
                    EngineEvent::RetiredAutomationLane(lane) => drop(lane),
                    EngineEvent::RetiredChannel(channel) => {
                        drop(channel);
                    }
                    EngineEvent::RoutingRetired(plan) => drop(plan),
                    EngineEvent::SidechainInputMeter {
                        effect_id,
                        input_id,
                        peak_l,
                        peak_r,
                    } => {
                        self.state
                            .devices
                            .sidechain_meters
                            .insert((effect_id, input_id), (peak_l, peak_r));
                    }
                    event @ (EngineEvent::ClipRecordArmed { .. }
                    | EngineEvent::ClipRecordStarted { .. }
                    | EngineEvent::ClipRecordStopped { .. }) => self.clip_record_event(event),
                    EngineEvent::DeviceProcessingFailed {
                        track_id,
                        effect_id,
                        reason,
                    } => {
                        self.state.status_text = format!(
                            "Device processing failed on {track_id:?} ({effect_id:?}): {reason}"
                        );
                    }
                    EngineEvent::DisposeEffect(cell) => {
                        // Plugin teardown remains on the UI thread.
                        drop(cell.take());
                    }
                    EngineEvent::DisposeInstrument(cell) => drop(cell.take()),
                    EngineEvent::PlaybackPosition(pos) => {
                        self.state.transport.position_samples = pos;
                    }
                    EngineEvent::PerformancePosition(pos) => {
                        self.state.perform.performance_position_samples = pos;
                    }
                    EngineEvent::Metering { peak_l, peak_r, .. } => {
                        self.state.peak_l = peak_l.max(self.state.peak_l * 0.85);
                        self.state.peak_r = peak_r.max(self.state.peak_r * 0.85);
                        let project_tracks = Arc::make_mut(&mut self.state.project_tracks);
                        project_tracks.master.peak_l = self.state.peak_l;
                        project_tracks.master.peak_r = self.state.peak_r;
                    }
                    EngineEvent::ClipQueued {
                        request_id,
                        track_id,
                        clip_id,
                    } => {
                        self.state
                            .perform
                            .clip_editor
                            .queue_request(track_id, clip_id, request_id);
                    }
                    EngineEvent::ClipEventsDropped { total } => {
                        self.state.status_text = format!("Clip events lost ({total}); launcher state will resync. Capture may be incomplete");
                    }
                    EngineEvent::ClipStateResynced(snapshot) => {
                        apply_clip_resync(&mut self.state, snapshot)
                    }
                    EngineEvent::ClipBatchRetired(retired) => drop(retired),
                    EngineEvent::ClipRequestRetired(retired) => {
                        self.state
                            .perform
                            .clip_editor
                            .pending
                            .remove(&retired.request_id);
                    }
                    EngineEvent::ClipSourceRefreshed {
                        track_id,
                        request_id,
                        position,
                        effective_at_samples,
                    } => {
                        let editor = &mut self.state.perform.clip_editor;
                        if let Some(clip) = editor.pending.remove(&request_id) {
                            let spb = self.state.transport.samples_per_beat();
                            let source = CapturedTimelineSource::from_clip(&clip, spb);
                            editor
                                .started_at
                                .insert(track_id, effective_at_samples.saturating_sub(position));
                            editor.playing.insert(track_id, clip);
                            self.state.perform.capture.clip_transition(
                                track_id,
                                Some(source),
                                effective_at_samples,
                                position,
                            );
                        }
                    }
                    EngineEvent::ClipCaptureSource {
                        track_id,
                        position,
                        effective_at_samples,
                    } => {
                        let spb = self.state.transport.samples_per_beat();
                        let source = self
                            .state
                            .perform
                            .clip_editor
                            .playing
                            .get(&track_id)
                            .map(|clip| CapturedTimelineSource::from_clip(clip, spb));
                        self.state.perform.capture.clip_transition(
                            track_id,
                            source,
                            effective_at_samples,
                            position,
                        );
                    }
                    EngineEvent::ClipTransitioned {
                        track_id,
                        request_id,
                        retired,
                        effective_at_samples,
                        ..
                    } => {
                        let editor = &mut self.state.perform.clip_editor;
                        editor.acknowledge_transition(track_id, request_id);
                        editor.started_at.insert(track_id, effective_at_samples);
                        if let Some(clip) = editor.pending.remove(&request_id) {
                            editor.playing.insert(track_id, clip);
                        } else {
                            editor.playing.remove(&track_id);
                        }
                        let spb = self.state.transport.samples_per_beat();
                        let source = editor
                            .playing
                            .get(&track_id)
                            .map(|clip| CapturedTimelineSource::from_clip(clip, spb));
                        self.state.perform.capture.clip_transition(
                            track_id,
                            source,
                            effective_at_samples,
                            0,
                        );
                        drop(retired);
                    }
                    EngineEvent::PlaybackStopped => {
                        self.state.perform.clip_editor.running = false;
                        self.state.perform.clip_editor.playing.clear();
                        self.state.perform.clip_editor.started_at.clear();
                        self.state.perform.clip_editor.clear_queue();
                        self.state.perform.clip_editor.pending.clear();
                        self.state.transport.playing = false;
                        self.state.perform.playing_section = None;
                        self.state.perform.queued_section = None;
                        self.state.perform.pending_section_boundary_samples = None;
                        self.state.perform.section_playhead_samples = 0;
                        self.state.perform.clear_pending_track_mutes();
                    }
                    EngineEvent::PlaybackStarted => {
                        self.state.transport.playing = true;
                    }
                    EngineEvent::AuditionStopped => {
                        self.state.browser.stop_audition_state();
                        if active_audition_status(&self.state.status_text) {
                            self.state.status_text = "Audition finished".into();
                        }
                    }
                    EngineEvent::AuditionQueued => {
                        self.state.browser.audition_loading = false;
                        self.state.browser.audition_playing = false;
                        self.state.browser.audition_queued = true;
                    }
                    EngineEvent::AuditionStarted => {
                        self.state.browser.audition_position_frames = 0;
                        self.state.browser.audition_queued = false;
                        self.state.browser.audition_playing = true;
                        let playback_mode = self
                            .state
                            .browser
                            .audition_playback_mode
                            .unwrap_or(self.state.browser.audition_mode);
                        let preparing_warp = playback_mode == AuditionMode::Raw
                            && self.state.browser.audition_mode == AuditionMode::Warp;
                        if !preparing_warp {
                            self.state.browser.audition_loading = false;
                        }
                        self.state.status_text = match playback_mode {
                            AuditionMode::Raw if preparing_warp => WARP_AUDITION_PREPARING.into(),
                            AuditionMode::Raw => RAW_AUDITION_PLAYING.into(),
                            AuditionMode::Warp => WARP_AUDITION_PLAYING.into(),
                        };
                    }
                    EngineEvent::AuditionPosition(position_frames) => {
                        self.state.browser.audition_position_frames = position_frames;
                    }
                    EngineEvent::TrackMeter {
                        track_id,
                        peak_l,
                        peak_r,
                    } => {
                        if let Some(track) = self.state.find_track_mut(track_id) {
                            track.peak_l = peak_l.max(track.peak_l * 0.85);
                            track.peak_r = peak_r.max(track.peak_r * 0.85);
                        }
                    }
                    EngineEvent::TrackNoteActivity { .. } => {}
                    EngineEvent::TrackMuteChanged {
                        track_id,
                        muted,
                        effective_at_samples,
                    } => {
                        self.state.perform.capture.track_mute_changed(
                            track_id,
                            muted,
                            effective_at_samples,
                        );
                        if apply_track_mute_event(&mut self.state, track_id, muted) {
                            self.save_runtime.project_changed(
                                self.state.auto_save_enabled,
                                self.state.project.current_path.is_some(),
                                std::time::Instant::now(),
                            );
                        }
                    }
                    EngineEvent::TrackMuteQueued {
                        track_id,
                        muted,
                        effective_at_samples,
                    } => {
                        self.state.perform.queue_track_mute_ui(
                            track_id,
                            muted,
                            effective_at_samples,
                        );
                    }
                    EngineEvent::TrackMuteQueueCancelled { track_id } => {
                        self.state.perform.cancel_track_mute_ui(track_id);
                        self.state.status_text = "Pending Track Mute cleared".into();
                    }
                    EngineEvent::AutomationOverrideChanged {
                        track_id,
                        target,
                        overridden,
                    } => {
                        self.state
                            .automation_ui
                            .set_override(track_id, target, overridden);
                    }
                    EngineEvent::AutomationGestureChanged {
                        track_id,
                        target,
                        normalized_value,
                        phase,
                        effective_at_samples,
                    } => {
                        self.state.perform.capture.automation_changed(
                            track_id,
                            target,
                            normalized_value,
                            phase,
                            effective_at_samples,
                        );
                    }
                    EngineEvent::NoteRepeated {
                        track_id,
                        pitch,
                        velocity,
                        rate,
                        effective_at_samples,
                        canonical_at_samples,
                        section_id,
                        canonical_section_position_samples,
                        ..
                    } => {
                        self.state.perform.clip_record.repeated_note(
                            track_id,
                            pitch,
                            velocity,
                            rate,
                            effective_at_samples,
                            canonical_at_samples,
                        );
                        self.state.perform.capture.repeated_note(
                            track_id,
                            pitch,
                            velocity,
                            rate,
                            effective_at_samples,
                            canonical_at_samples,
                        );
                        self.state.perform.section_record.repeated_note(
                            section_id,
                            track_id,
                            pitch,
                            velocity,
                            rate,
                            effective_at_samples,
                            canonical_section_position_samples,
                        );
                    }
                    EngineEvent::InstrumentNoteInput {
                        track_id,
                        pitch,
                        velocity,
                        on,
                        effective_at_samples,
                        section_id,
                        section_position_samples,
                    } => {
                        self.state.perform.clip_record.note_input(
                            track_id,
                            pitch,
                            velocity,
                            on,
                            effective_at_samples,
                        );
                        self.state.perform.capture.input_note(
                            track_id,
                            pitch,
                            velocity,
                            on,
                            effective_at_samples,
                        );
                        self.state.perform.section_record.input_note(
                            crate::domains::perform::section_record::SectionRecordInput {
                                target_id: section_id,
                                track_id,
                                pitch,
                                velocity,
                                on,
                                effective_at_samples,
                                local_position_samples: section_position_samples,
                            },
                        );
                    }
                    EngineEvent::SectionRecordArmed {
                        section_id,
                        track_id,
                        effective_at_samples,
                        ..
                    } => {
                        self.state.perform.section_record.arm(
                            section_id,
                            track_id,
                            effective_at_samples,
                        );
                        self.state.status_text =
                            format!("Section Record pending at sample {effective_at_samples}");
                    }
                    EngineEvent::SectionRecordStarted {
                        section_id,
                        track_id,
                        effective_at_samples,
                        section_position_samples,
                    } => {
                        self.state.perform.section_record.start(
                            section_id,
                            track_id,
                            effective_at_samples,
                            section_position_samples,
                        );
                        self.state.status_text = "Section Record running".into();
                    }
                    EngineEvent::SectionRecordStopped {
                        section_id,
                        track_id,
                        effective_at_samples,
                        section_position_samples,
                        started,
                        retired,
                    } => {
                        let completed = self.state.perform.section_record.finish(
                            section_id,
                            track_id,
                            effective_at_samples,
                            section_position_samples,
                            started,
                        );
                        drop(retired);
                        completed_section_recordings.push(completed);
                    }
                    EngineEvent::PerformanceCaptureStarted {
                        effective_at_samples,
                        section_id,
                        section_position_samples,
                    } => {
                        let active = section_id.zip(section_position_samples).and_then(
                            |(section_id, position)| {
                                self.state
                                    .perform
                                    .sections
                                    .by_id(section_id)
                                    .map(|section| {
                                        (CapturedTimelineSource::from_section(section), position)
                                    })
                            },
                        );
                        self.state
                            .perform
                            .capture
                            .start(effective_at_samples, active);
                        self.state.status_text = "Capture recording into Arrange".into();
                    }
                    EngineEvent::PerformanceCaptureStopped {
                        effective_at_samples,
                    } => {
                        if self.state.perform.capture.is_active() {
                            completed_captures
                                .push(self.state.perform.capture.finish(effective_at_samples));
                        }
                    }
                    EngineEvent::SectionTransitioned {
                        section_id,
                        effective_at_samples,
                        retired,
                    } => {
                        let captured_source = self
                            .state
                            .perform
                            .sections
                            .by_id(section_id)
                            .map(CapturedTimelineSource::from_section);
                        if let Some(source) = captured_source {
                            self.state
                                .perform
                                .capture
                                .transition(source, effective_at_samples);
                        }
                        self.state.perform.playing_section = Some(section_id);
                        self.state.perform.queued_section = None;
                        self.state.perform.pending_section_boundary_samples = None;
                        self.state.perform.section_playhead_samples = 0;
                        self.state.status_text =
                            format!("Section playing at sample {effective_at_samples}");
                        drop(retired);
                    }
                    EngineEvent::SectionQueued {
                        section_id,
                        effective_at_samples,
                        retired,
                    } => {
                        self.state.perform.queued_section = Some(section_id);
                        self.state.perform.pending_section_boundary_samples =
                            Some(effective_at_samples);
                        drop(retired);
                    }
                    EngineEvent::SectionQueueCancelled { retired } => {
                        self.state.perform.queued_section = None;
                        self.state.perform.pending_section_boundary_samples = None;
                        drop(retired);
                    }
                    EngineEvent::SectionPlaybackPosition {
                        section_id,
                        position_samples,
                    } => {
                        if self.state.perform.playing_section == Some(section_id) {
                            self.state.perform.section_playhead_samples = position_samples;
                        }
                        self.state
                            .perform
                            .section_record
                            .observe_playhead(section_id, position_samples);
                    }
                    EngineEvent::SectionSourceRefreshed {
                        section_id,
                        applied,
                        effective_at_samples,
                        section_position_samples,
                        retired,
                    } => {
                        if applied {
                            if let Some(source) = self
                                .state
                                .perform
                                .sections
                                .by_id(section_id)
                                .map(CapturedTimelineSource::from_section)
                            {
                                self.state.perform.capture.refresh(
                                    source,
                                    effective_at_samples,
                                    section_position_samples.unwrap_or(0),
                                );
                            }
                        }
                        drop(retired);
                    }
                }
            }
        }
        self.refresh_clip_record(self.state.perform.performance_position_samples);
        for completed in completed_section_recordings {
            self.finish_section_record_session(completed);
        }
        for completed in completed_captures {
            self.finish_performance_capture(completed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::perform::PendingTrackMute;
    use crate::state::ProjectTrack;
    use vibez_core::id::TrackId;

    #[test]
    fn dropped_engine_transition_recovers_the_actual_launcher_state() {
        use crate::domains::perform::clip_record::empty_midi_clip;
        use vibez_core::id::ClipId;
        use vibez_core::perform::MusicalBoundary;
        use vibez_engine::commands::EngineCommand;
        let (mut engine, mut commands, mut events) = vibez_engine::engine::AudioEngine::new();
        let track = TrackId::new();
        commands
            .push(EngineCommand::AddTrack(track, "Bass".into()))
            .unwrap();
        commands.push(EngineCommand::SetSampleRate(8)).unwrap();
        commands.push(EngineCommand::SetBpm(120.0)).unwrap();
        for _ in 0..vibez_core::constants::RING_BUFFER_CAPACITY {
            engine.process_block(vibez_engine::engine::AudioProcessBlock::new(&mut [], 1));
        }
        let clip = empty_midi_clip(ClipId::new(), track, 0, "Take".into(), 4.0);
        let mut state = AppState::default();
        state.perform.layout = vibez_project::PerformLayout::Clips;
        Arc::make_mut(&mut state.perform.clips)
            .clips
            .push(clip.clone());
        state.perform.clip_editor.pending.insert(1, clip.clone());
        state.perform.clip_editor.next_request = 1;
        state
            .perform
            .clip_editor
            .queue_request(track, Some(clip.id), 1);
        state.perform.clip_editor.running = true;
        commands
            .push(EngineCommand::QueueClips {
                clips: vec![clip.prepare(1, 4.0)],
                quantization: MusicalBoundary::Immediate,
            })
            .unwrap();
        engine.process_block(vibez_engine::engine::AudioProcessBlock::new(
            &mut [0.0; 1],
            1,
        ));
        let mut observed_drop = false;
        for _ in 0..4 {
            while let Ok(event) = events.pop() {
                match event {
                    EngineEvent::ClipEventsDropped { total } => observed_drop |= total > 0,
                    EngineEvent::ClipStateResynced(snapshot) => {
                        apply_clip_resync(&mut state, snapshot)
                    }
                    EngineEvent::ClipTransitioned { .. } => {
                        panic!("transition should have been lost")
                    }
                    _ => {}
                }
            }
            engine.process_block(vibez_engine::engine::AudioProcessBlock::new(
                &mut [0.0; 1],
                1,
            ));
        }
        assert!(observed_drop);
        assert_eq!(state.perform.clip_editor.playing[&track].id, clip.id);
        assert!(state.perform.clip_editor.queued.is_empty());
        assert!(state.perform.clip_editor.pending.is_empty());
        assert!(state.perform.clip_editor.running);
        assert!(
            state.transport.playing,
            "a lost PlaybackStarted must also recover"
        );
        assert!(state.perform.clip_editor.started_at.contains_key(&track));
    }

    #[test]
    fn resync_restarts_capture_from_the_recovered_local_position() {
        use vibez_core::id::ClipId;
        use vibez_engine::events::{ClipPlayingState, ClipTrackState};
        let track = TrackId::new();
        let mut clip = crate::domains::perform::clip_record::empty_midi_clip(
            ClipId::new(),
            track,
            0,
            "Take".into(),
            4.0,
        );
        Arc::make_mut(&mut clip.timeline).ensure(track).note_clips[0]
            .notes
            .push(vibez_core::midi::MidiNote {
                pitch: 42,
                velocity: 100,
                start_beat: 1.0,
                duration_beats: 0.5,
            });
        let mut state = AppState::default();
        state.transport.sample_rate = 8;
        state.transport.bpm = 120.0;
        state.perform.capture.phase = crate::domains::perform::CapturePhase::Starting;
        state.perform.capture.prepare(0, 8, 120.0);
        state.perform.capture.start(0, None);
        state.perform.clip_editor.pending.insert(1, clip.clone());
        state.perform.clip_editor.next_request = 1;
        apply_clip_resync(
            &mut state,
            ClipTrackState {
                track_id: track,
                playing: Some(ClipPlayingState {
                    clip_id: clip.id,
                    request_id: 1,
                    position: 4,
                }),
                queued: None,
                through_request: 1,
                effective_at_samples: 8,
                running: true,
                transport_playing: true,
            },
        );
        let captured = state.perform.capture.finish(12).unwrap().materialize();
        let notes = &captured.by_track[&track].note_clips[0].notes;
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].pitch, 42);
        assert_eq!(captured.by_track[&track].note_clips[0].position_beats, 2.0);
    }

    #[test]
    fn audition_completion_recognizes_every_canonical_playing_status() {
        for status in [
            RAW_AUDITION_PLAYING,
            WARP_AUDITION_PLAYING,
            WARP_AUDITION_PREPARING,
        ] {
            assert!(active_audition_status(status));
        }
        assert!(!active_audition_status("Audition unavailable"));
    }

    #[test]
    fn effective_quantized_mute_commits_one_undoable_project_edit() {
        let track_id = TrackId::new();
        let mut state = AppState::default();
        Arc::make_mut(&mut state.project_tracks)
            .tracks
            .push(ProjectTrack::new(track_id, "Bass".into(), 0));
        state.perform.queue_track_mute_ui(track_id, true, 48_000);

        assert!(apply_track_mute_event(&mut state, track_id, true));
        assert!(state.project_tracks.tracks[0].mute);
        assert_eq!(state.project.history.undo.len(), 1);
        let before = state.project.history.pop_undo().expect("mute undo");
        assert!(!before.project_tracks.tracks[0].mute);
        assert_eq!(
            state.perform.pending_track_mute(track_id),
            None::<PendingTrackMute>
        );
    }

    #[test]
    fn every_drum_pad_feedback_event_path_drives_the_same_flash_state() {
        use vibez_core::perform::NoteRepeatRate;

        let track_id = TrackId::new();
        let now = std::time::Instant::now();
        let mut view = crate::state::ViewState::default();
        let clip_activity = EngineEvent::TrackNoteActivity {
            track_id,
            triggered_notes: (1u128 << 41) | (1u128 << 44),
        };
        let repeated = EngineEvent::NoteRepeated {
            track_id,
            pitch: 42,
            velocity: 100,
            rate: NoteRepeatRate::Eighth,
            effective_at_samples: 0,
            canonical_at_samples: 0,
            section_id: None,
            section_position_samples: None,
            canonical_section_position_samples: None,
        };
        let input = EngineEvent::InstrumentNoteInput {
            track_id,
            pitch: 43,
            velocity: 100,
            on: true,
            effective_at_samples: 0,
            section_id: None,
            section_position_samples: None,
        };
        let input_note_off = EngineEvent::InstrumentNoteInput {
            track_id,
            pitch: 45,
            velocity: 0,
            on: false,
            effective_at_samples: 0,
            section_id: None,
            section_position_samples: None,
        };

        apply_drum_pad_flash(&mut view, &clip_activity, now);
        apply_drum_pad_flash(&mut view, &repeated, now);
        apply_drum_pad_flash(&mut view, &input, now);
        apply_drum_pad_flash(&mut view, &input_note_off, now);

        assert!(view.drum_pad_is_flashing(track_id, 41, now));
        assert!(view.drum_pad_is_flashing(track_id, 42, now));
        assert!(view.drum_pad_is_flashing(track_id, 43, now));
        assert!(view.drum_pad_is_flashing(track_id, 44, now));
        assert!(!view.drum_pad_is_flashing(track_id, 45, now));
    }
}
