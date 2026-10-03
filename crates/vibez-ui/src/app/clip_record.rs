//! Clip recording coordinates shared input and note capture without creating Sections.
use super::*;
use crate::domains::perform::clip_record::*;
use crate::domains::perform::loop_record::LoopRecordMode;
use crate::domains::perform::LauncherClip;
use crate::message::{AudioRecordingOutcome, Message};
use iced::Task;
use std::sync::Arc;
use vibez_core::id::{ClipId, TrackId};
use vibez_engine::commands::EngineCommand;

impl App {
    pub(super) fn apply_clip_record_action(&mut self, action: ClipRecordAction) -> Task<Message> {
        match action {
            ClipRecordAction::Start { track_id, row } => self.begin_clip_record(track_id, row),
            ClipRecordAction::Stop
                if self.state.perform.clip_record.pending_audio_arm.is_some() =>
            {
                self.cancel_clip_record();
                Task::none()
            }
            ClipRecordAction::Stop => {
                self.send_command(EngineCommand::StopClipRecord { immediate: false });
                Task::none()
            }
        }
    }

    fn begin_clip_record(&mut self, track_id: TrackId, row: u32) -> Task<Message> {
        if self.state.audio_recording.is_busy() || self.state.perform.capture.is_active() {
            self.state.status_text =
                "Finish the current recording before starting a Clip take".into();
            return Task::none();
        }
        if !matches!(self.state.audio_stream_health, AudioStreamHealth::Running) {
            self.state.status_text = "Clip recording needs a running Audio Output clock".into();
            return Task::none();
        }
        let Some(track) = self.state.find_track(track_id) else {
            return Task::none();
        };
        let audio = !track.kind.is_midi();
        if !audio && !track.is_playable_midi_target() {
            self.state.status_text = "Load an instrument on this Track before recording".into();
            return Task::none();
        }
        let name = default_clip_name(&track.name, row);
        let original = self.state.perform.clips.at(track_id, row).cloned();
        let spb = self.state.transport.samples_per_beat();
        let length = original
            .as_ref()
            .map(|clip| clip.length_and_loop(spb).0)
            .or_else(|| {
                self.state
                    .perform
                    .clip_record
                    .length
                    .beats()
                    .map(|beats| (beats * spb).round() as u64)
            });
        let id = original.as_ref().map_or_else(ClipId::new, |clip| clip.id);
        let beats = length.map_or(16.0, |samples| samples as f64 / spb);
        let working = if audio {
            original.clone().unwrap_or(LauncherClip {
                id,
                track_id,
                row,
                timeline: Arc::default(),
            })
        } else {
            original
                .as_ref()
                .map(|clip| normalize_midi_clip(clip, beats))
                .unwrap_or_else(|| empty_midi_clip(id, track_id, row, name, beats))
        };
        if audio {
            self.state.audio_recording.arm(track_id);
            self.state.audio_recording.monitor_track = Some(track_id);
            if let Err(error) = self.sync_audio_input_runtime() {
                self.rollback_clip_audio_arm();
                self.state.status_text = format!("Audio input could not open: {error}");
                return Task::none();
            }
        }
        if !self.begin_project_transaction() {
            if audio {
                self.rollback_clip_audio_arm();
            }
            self.state.status_text = "Finish the current edit before recording".into();
            return Task::none();
        }
        if audio {
            let Some(source) = self.recording_source_for_target(track_id) else {
                self.discard_project_transaction();
                self.rollback_clip_audio_arm();
                self.state.status_text = "Choose an available Audio input for this Track".into();
                return Task::none();
            };
            self.state.audio_recording.begin(0, source);
            self.input_bridge.begin_output_clock_recording();
        }
        let record = &mut self.state.perform.clip_record;
        record.begin_session(
            ClipRecordSession {
                original,
                working: working.clone(),
                audio,
                length_samples: length,
                start: None,
                output_start: None,
                pass: 0,
                stop: None,
                sample_rate: self.state.transport.sample_rate,
                bpm: self.state.transport.bpm,
            },
            self.state.transport.playing,
        );
        let count_in_bars = record.notes.count_in.bars();
        let mode = if audio {
            LoopRecordMode::Replace
        } else {
            record.notes.mode
        };
        let mut prepared = working.prepare(0, spb);
        if audio || mode == LoopRecordMode::Replace {
            prepared.source = Box::default();
        }
        prepared.length_samples = length.unwrap_or(u64::MAX / 4);
        prepared.looping = length.is_some();
        self.state.perform.clip_editor.next_request += 1;
        prepared.request_id = self.state.perform.clip_editor.next_request;
        self.state
            .perform
            .clip_editor
            .pending
            .insert(prepared.request_id, working.clone());
        if self.state.perform.clips.by_id(id).is_none() {
            Arc::make_mut(&mut self.state.perform.clips)
                .clips
                .push(working);
        }
        self.state.perform.select_launcher_clip(id);
        self.state.arrangement.selected_track = Some(track_id);
        self.state.perform.sync_instrument_target_from_selection(
            Some(track_id),
            &self.state.project_tracks.tracks,
        );
        self.state.perform.clip_editor.running = true;
        if audio {
            self.state.perform.clip_record.pending_audio_arm = Some(prepared);
        } else {
            self.send_command(EngineCommand::ArmClipRecord {
                free_length: length.is_none(),
                prepared,
                count_in_bars,
            });
        }
        self.state.status_text = "Clip recording armed".into();
        Task::none()
    }

    pub(super) fn clip_record_event(&mut self, event: vibez_engine::events::EngineEvent) {
        use vibez_engine::events::EngineEvent;
        match event {
            EngineEvent::ClipRecordArmed {
                clip_id,
                track_id,
                start,
                output_start,
            } => {
                if let Some(session) = self
                    .state
                    .perform
                    .clip_record
                    .session
                    .as_mut()
                    .filter(|s| s.working.id == clip_id)
                {
                    session.output_start = Some(output_start);
                    self.state
                        .perform
                        .clip_record
                        .notes
                        .arm(clip_id, track_id, start);
                }
            }
            EngineEvent::ClipRecordStarted { clip_id, at } => {
                if let Some(session) = self
                    .state
                    .perform
                    .clip_record
                    .session
                    .as_mut()
                    .filter(|s| s.working.id == clip_id)
                {
                    session.start = Some(at);
                    self.state.perform.clip_record.notes.start(
                        clip_id,
                        session.working.track_id,
                        at,
                        0,
                    );
                    self.state.status_text = "Recording into Clip".into();
                }
            }
            EngineEvent::ClipRecordStopped {
                clip_id,
                at,
                started,
            } => {
                if self
                    .state
                    .perform
                    .clip_record
                    .session
                    .as_ref()
                    .is_none_or(|s| s.working.id != clip_id)
                {
                    return;
                }
                if !started {
                    self.cancel_clip_record();
                    return;
                }
                self.refresh_clip_record(at);
                let session = self
                    .state
                    .perform
                    .clip_record
                    .session
                    .as_mut()
                    .expect("record target");
                session.stop = Some(at);
                if session.audio {
                    self.state.audio_recording.request_stop();
                    self.input_bridge.end_recording();
                } else {
                    self.commit_clip_record();
                }
            }
            _ => {}
        }
    }

    pub(super) fn refresh_clip_record(&mut self, now: u64) {
        if let Some(clip) = self.state.perform.clip_record.preview(now) {
            self.publish_recorded_clip(clip, true);
        }
    }

    pub(super) fn publish_recorded_clip(&mut self, clip: LauncherClip, audible: bool) {
        let spb = self.state.transport.samples_per_beat();
        if let Some(target) = Arc::make_mut(&mut self.state.perform.clips).by_id_mut(clip.id) {
            *target = clip.clone();
        }
        if self
            .state
            .perform
            .clip_editor
            .playing
            .get(&clip.track_id)
            .is_some_and(|active| active.id == clip.id)
        {
            self.state
                .perform
                .clip_editor
                .playing
                .insert(clip.track_id, clip.clone());
        }
        if self.state.perform.clip_editor.selected == Some(clip.id) {
            self.state.perform.clip_editor.editor.timeline = Arc::clone(&clip.timeline);
        }
        if audible {
            let mut prepared = clip.prepare(0, spb);
            if self
                .state
                .perform
                .clip_record
                .session
                .as_ref()
                .is_some_and(|s| s.working.id == clip.id && s.length_samples.is_some())
            {
                prepared.looping = true;
            }
            self.send_command(EngineCommand::RefreshClip(prepared));
        }
    }

    pub(super) fn cancel_clip_record(&mut self) {
        let audio = {
            let mut engine = crate::domains::EngineTx(&mut self.cmd_tx);
            self.state.perform.cancel_clip_record(&mut engine)
        };
        let Some(audio) = audio else {
            return;
        };
        if audio {
            self.input_bridge.end_recording();
            self.finish_clip_audio_input();
        }
        self.discard_project_transaction();
        self.state.status_text = "Clip recording cancelled".into();
    }

    fn commit_clip_record(&mut self) {
        let Some(session) = self.state.perform.clip_record.finish_session() else {
            return;
        };
        if let Some(clip) = self.state.perform.clips.by_id(session.working.id).cloned() {
            self.publish_recorded_clip(clip, self.state.transport.playing);
        }
        self.push_undo_snapshot(None);
        self.mark_project_dirty();
        self.commit_project_transaction();
        self.state.status_text = "Clip recorded · one undo step".into();
    }

    fn rollback_clip_audio_arm(&mut self) {
        self.state.audio_recording.disarm();
        self.state.audio_recording.monitor_track = self.persisted_monitor_on_track();
        let _ = self.sync_audio_input_runtime();
    }

    fn finish_clip_audio_input(&mut self) {
        self.state.audio_recording.finish();
        self.rollback_clip_audio_arm();
    }

    pub(super) fn finish_clip_audio_recording(
        &mut self,
        outcome: AudioRecordingOutcome,
    ) -> Task<Message> {
        let Some(session) = self.state.perform.clip_record.session.as_ref() else {
            return Task::none();
        };
        let id = session.working.id;
        let mut clip = super::audio_recording::audio_outcome_clip(&outcome, id);
        clip.name = session
            .original
            .as_ref()
            .map(|clip| clip.name().to_owned())
            .unwrap_or_else(|| {
                default_clip_name(
                    self.state
                        .find_track(session.working.track_id)
                        .map_or("Audio", |track| track.name.as_str()),
                    session.working.row,
                )
            });
        clip.position = 0;
        clip.loop_enabled = session
            .original
            .as_ref()
            .is_none_or(|clip| clip.length_and_loop(session.samples_per_beat()).1);
        let mut timeline = crate::state::ArrangementTimeline::default();
        timeline.ensure(outcome.track_id).clips.push(clip);
        let recorded = LauncherClip {
            id,
            track_id: outcome.track_id,
            row: session.working.row,
            timeline: Arc::new(timeline),
        };
        self.publish_recorded_clip(recorded, self.state.transport.playing);
        self.state.perform.select_launcher_clip(id);
        self.finish_clip_audio_input();
        self.commit_clip_record();
        if let Some(warning) = outcome.quality_warning {
            self.state.status_text = warning;
        }
        Task::none()
    }
}
