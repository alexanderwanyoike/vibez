//! Clip recording coordinates shared input and note capture without creating Sections.
use super::*;
use crate::domains::perform::clip_record::*;
use crate::domains::perform::loop_record::{LoopRecordInput, LoopRecordMode};
use crate::domains::perform::LauncherClip;
use crate::message::{AudioRecordingOutcome, Message};
use iced::Task;
use std::sync::Arc;
use vibez_core::id::{ClipId, TrackId};
use vibez_engine::commands::EngineCommand;

impl App {
    pub(super) fn update_clip_record(&mut self, msg: ClipRecordMsg) -> Task<Message> {
        if self.state.perform.layout != vibez_project::PerformLayout::Clips {
            return Task::none();
        }
        if let ClipRecordMsg::Slot(track, row) = msg {
            if let Some(session) = &self.state.perform.clip_record.session {
                if (session.working.track_id, session.working.row) == (track, row)
                    && session.stop.is_none()
                {
                    if self.state.perform.clip_record.notes.request_stop() {
                        self.send_command(EngineCommand::StopClipRecord { immediate: false });
                    }
                } else {
                    self.state.status_text = "Finish the current Clip take first".into();
                }
                return Task::none();
            }
            return self.begin_clip_record(track, row);
        }
        if !self.state.perform.clip_record.is_active() {
            let record = &mut self.state.perform.clip_record;
            match msg {
                ClipRecordMsg::SetLength(length) => record.length = length,
                ClipRecordMsg::SetMode(mode) => record.notes.mode = mode,
                ClipRecordMsg::SetCountIn(count) => record.notes.count_in = count,
                ClipRecordMsg::SetQuantization(quantization) => {
                    record.notes.quantization = quantization
                }
                _ => {}
            }
        }
        Task::none()
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
        let name = format!("{} {}", track.name, row + 1);
        let original = self.state.perform.clips.at(track_id, row).cloned();
        let spb = self.state.transport.sample_rate as f64 * 60.0 / self.state.transport.bpm;
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
                self.state.audio_recording.disarm();
                self.state.audio_recording.monitor_track = None;
                self.sync_audio_input_target();
                self.state.status_text = format!("Audio input could not open: {error}");
                return Task::none();
            }
        }
        if !self.begin_project_transaction() {
            if audio {
                self.state.audio_recording.disarm();
                self.state.audio_recording.monitor_track = None;
                self.sync_audio_input_target();
            }
            self.state.status_text = "Finish the current edit before recording".into();
            return Task::none();
        }
        if audio {
            let Some(source) = self.recording_source_for_target(track_id) else {
                self.state.project.history.abandon_transaction();
                self.state.audio_recording.disarm();
                self.state.audio_recording.monitor_track = None;
                self.sync_audio_input_target();
                self.state.status_text = "Choose an available Audio input for this Track".into();
                return Task::none();
            };
            self.state.audio_recording.begin(0, source);
            self.input_bridge.begin_output_clock_recording();
        }
        let record = &mut self.state.perform.clip_record;
        record.notes.sync_clock(
            self.state.transport.playing,
            self.state.transport.bpm,
            self.state.transport.sample_rate,
        );
        record.notes.request_start(
            id,
            track_id,
            !self.state.transport.playing,
            length.map_or(1_000_000.0, |samples| samples as f64 / spb),
        );
        record.notes.mark_arm_sent();
        let count_in_bars = record.notes.count_in.bars();
        let mode = if audio {
            LoopRecordMode::Replace
        } else {
            record.notes.mode
        };
        record.session = Some(ClipRecordSession {
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
        });
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
        self.send_command(EngineCommand::ArmClipRecord {
            free_length: length.is_none(),
            prepared,
            count_in_bars,
        });
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
            EngineEvent::InstrumentNoteInput {
                track_id,
                pitch,
                velocity,
                on,
                effective_at_samples,
                ..
            } => {
                self.refresh_clip_record_pass(effective_at_samples);
                let Some(session) = self
                    .state
                    .perform
                    .clip_record
                    .session
                    .as_ref()
                    .filter(|s| !s.audio && s.working.track_id == track_id)
                else {
                    return;
                };
                let Some(start) = session.start else {
                    return;
                };
                let local = record_local_position(session, effective_at_samples);
                if effective_at_samples >= start {
                    self.state
                        .perform
                        .clip_record
                        .notes
                        .input_note(LoopRecordInput {
                            target_id: Some(session.working.id),
                            track_id,
                            pitch,
                            velocity,
                            on,
                            effective_at_samples,
                            local_position_samples: Some(local),
                        });
                }
            }
            EngineEvent::NoteRepeated {
                track_id,
                pitch,
                velocity,
                rate,
                effective_at_samples,
                canonical_at_samples,
                ..
            } => {
                self.refresh_clip_record_pass(effective_at_samples);
                let Some(session) = self
                    .state
                    .perform
                    .clip_record
                    .session
                    .as_ref()
                    .filter(|s| !s.audio && s.working.track_id == track_id)
                else {
                    return;
                };
                let local = record_local_position(session, canonical_at_samples);
                self.state.perform.clip_record.notes.repeated_note(
                    Some(session.working.id),
                    track_id,
                    pitch,
                    velocity,
                    rate,
                    effective_at_samples,
                    Some(local),
                );
            }
            _ => {}
        }
    }

    fn refresh_clip_record_pass(&mut self, now: u64) {
        let Some(session) = self
            .state
            .perform
            .clip_record
            .session
            .as_mut()
            .filter(|s| !s.audio)
        else {
            return;
        };
        let (Some(start), Some(length)) = (session.start, session.length_samples) else {
            return;
        };
        while now >= start.saturating_add((session.pass + 1).saturating_mul(length)) {
            let boundary = start + (session.pass + 1) * length;
            if let Some(take) = self.state.perform.clip_record.notes.next_pass(boundary) {
                session.working = apply_midi_take(
                    &session.working,
                    &take,
                    length as f64 * session.bpm / (session.sample_rate as f64 * 60.0),
                );
            }
            session.pass += 1;
        }
    }

    pub(super) fn refresh_clip_record(&mut self, now: u64) {
        self.refresh_clip_record_pass(now);
        let Some(session) = self
            .state
            .perform
            .clip_record
            .session
            .as_ref()
            .filter(|s| !s.audio && s.start.is_some() && s.stop.is_none())
        else {
            return;
        };
        let local = record_local_position(session, now);
        let Some(take) = self.state.perform.clip_record.notes.snapshot(now, local) else {
            return;
        };
        let spb = session.sample_rate as f64 * 60.0 / session.bpm;
        let beats = session.length_samples.map_or_else(
            || free_recording_beats(now.saturating_sub(session.start.unwrap()), spb),
            |length| length as f64 / spb,
        );
        let clip = apply_midi_take(&session.working, &take, beats);
        self.publish_recorded_clip(clip, true);
    }

    pub(super) fn publish_recorded_clip(&mut self, clip: LauncherClip, audible: bool) {
        let spb = self.state.transport.sample_rate as f64 * 60.0 / self.state.transport.bpm;
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
        let Some(session) = self.state.perform.clip_record.session.take() else {
            return;
        };
        let store = Arc::make_mut(&mut self.state.perform.clips);
        store.clips.retain(|clip| clip.id != session.working.id);
        let id = session.working.id;
        if let Some(original) = session.original {
            store.clips.push(original);
            self.state.perform.select_launcher_clip(id);
        } else if self.state.perform.clip_editor.selected == Some(id) {
            self.state.perform.clip_editor.selected = None;
            self.state.perform.clip_editor.editor = Default::default();
        }
        self.state.perform.clip_record.notes.cancel();
        if session.audio {
            self.input_bridge.end_recording();
            self.finish_clip_audio_input();
        }
        if let Some((_, dirty)) = self.state.project.history.abandon_transaction() {
            self.state.project.dirty = dirty;
        }
        self.state.status_text = "Clip recording cancelled".into();
    }

    fn commit_clip_record(&mut self) {
        let Some(session) = self.state.perform.clip_record.session.take() else {
            return;
        };
        self.state.perform.clip_record.notes.cancel();
        if let Some(clip) = self.state.perform.clips.by_id(session.working.id).cloned() {
            self.publish_recorded_clip(clip, self.state.transport.playing);
        }
        self.push_undo_snapshot(None);
        self.mark_project_dirty();
        self.commit_project_transaction();
        self.state.status_text = "Clip recorded · one undo step".into();
    }

    fn finish_clip_audio_input(&mut self) {
        self.state.audio_recording.finish();
        self.state.audio_recording.disarm();
        self.state.audio_recording.monitor_track = self.persisted_monitor_on_track();
        let _ = self.sync_audio_input_runtime();
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
                format!(
                    "{} {}",
                    self.state
                        .find_track(session.working.track_id)
                        .map_or("Audio", |track| track.name.as_str()),
                    session.working.row + 1
                )
            });
        clip.position = 0;
        clip.loop_enabled = session.original.as_ref().is_none_or(|clip| {
            clip.length_and_loop(session.sample_rate as f64 * 60.0 / session.bpm)
                .1
        });
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

fn record_local_position(session: &ClipRecordSession, now: u64) -> u64 {
    let elapsed = now.saturating_sub(session.start.unwrap_or(now));
    session
        .length_samples
        .map_or(elapsed, |length| elapsed % length.max(1))
}
