//! UI-thread consumption of audio-engine events.

use std::sync::Arc;

use vibez_engine::events::EngineEvent;

use crate::domains::perform::CapturedSectionSource;
use crate::state::AuditionMode;

use super::*;

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
                    }
                    EngineEvent::PlaybackStarted => {
                        self.state.transport.playing = true;
                    }
                    EngineEvent::AuditionStopped => {
                        self.state.browser.stop_audition_state();
                        if matches!(
                            self.state.status_text.as_str(),
                            "RAW Audition playing" | "WARP Audition playing"
                        ) {
                            self.state.status_text = "Audition finished".into();
                        }
                    }
                    EngineEvent::AuditionQueued => {
                        self.state.browser.audition_loading = false;
                        self.state.browser.audition_playing = false;
                        self.state.browser.audition_queued = true;
                    }
                    EngineEvent::AuditionStarted => {
                        self.state.browser.audition_loading = false;
                        self.state.browser.audition_queued = false;
                        self.state.browser.audition_playing = true;
                        self.state.status_text = match self.state.browser.audition_mode {
                            AuditionMode::Raw => "RAW Audition playing".into(),
                            AuditionMode::Warp => "WARP Audition playing".into(),
                        };
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
                        effective_at_samples: _,
                    } => {
                        if let Some(track) = self.state.find_track_mut(track_id) {
                            track.mute = muted;
                        }
                    }
                    EngineEvent::NoteRepeated {
                        track_id,
                        pitch,
                        velocity,
                        rate,
                        effective_at_samples,
                        section_id,
                        canonical_section_position_samples,
                        ..
                    } => {
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
                        section_id: _,
                        applied: _,
                        retired,
                    } => drop(retired),
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
