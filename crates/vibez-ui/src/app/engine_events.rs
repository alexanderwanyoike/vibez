//! UI-thread consumption of audio-engine events.

use std::sync::Arc;

use vibez_engine::events::EngineEvent;

use crate::domains::perform::CapturedSectionSource;
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

impl App {
    pub(super) fn poll_engine_events(&mut self) {
        let mut completed_section_recordings = Vec::new();
        let mut completed_captures = Vec::new();
        if let Some(ref mut rx) = self.event_rx {
            while let Ok(event) = rx.pop() {
                match event {
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
                    EngineEvent::PlaybackStopped => {
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
                        self.state.perform.capture.input_note(
                            track_id,
                            pitch,
                            velocity,
                            on,
                            effective_at_samples,
                        );
                        self.state.perform.section_record.input_note(
                            crate::domains::perform::section_record::SectionRecordInput {
                                section_id,
                                track_id,
                                pitch,
                                velocity,
                                on,
                                effective_at_samples,
                                section_position_samples,
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
                                        (CapturedSectionSource::from_section(section), position)
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
                            .map(CapturedSectionSource::from_section);
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
                                .map(CapturedSectionSource::from_section)
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
}
