//! Clip takes own pass advancement, preview publication and cancellation state.

use super::*;
use crate::domains::{perform::loop_record::LoopRecordInput, EngineHandle};
use vibez_core::perform::NoteRepeatRate;
use vibez_engine::commands::EngineCommand;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PreviewStamp {
    notes: (usize, usize),
    pass: u64,
    length: u64,
    held_until: Option<u64>,
    replaced: usize,
}

impl ClipRecordSession {
    pub fn samples_per_beat(&self) -> f64 {
        vibez_core::time::TempoMap::new(self.bpm, self.sample_rate).samples_per_beat()
    }

    pub fn local_position(&self, now: u64) -> u64 {
        let elapsed = now.saturating_sub(self.start.unwrap_or(now));
        self.length_samples
            .map_or(elapsed, |length| elapsed % length.max(1))
    }
}

impl ClipRecordState {
    pub fn begin_session(&mut self, session: ClipRecordSession, playing: bool) {
        self.last_preview = None;
        self.notes
            .sync_clock(playing, session.bpm, session.sample_rate);
        self.notes.request_start(
            session.working.id,
            session.working.track_id,
            !playing,
            session.length_samples.map_or(1_000_000.0, |length| {
                length as f64 / session.samples_per_beat()
            }),
        );
        if !session.audio {
            self.notes.mark_arm_sent();
        }
        self.session = Some(session);
    }

    pub fn arm_audio_when_ready(&mut self, bridge_start: Option<u64>) -> Option<EngineCommand> {
        bridge_start?;
        let session = self.session.as_ref()?;
        let prepared = self.pending_audio_arm.take()?;
        self.notes.mark_arm_sent();
        Some(EngineCommand::ArmClipRecord {
            free_length: session.length_samples.is_none(),
            prepared,
            count_in_bars: self.notes.count_in.bars(),
        })
    }

    pub fn finish_session(&mut self) -> Option<ClipRecordSession> {
        self.last_preview = None;
        self.pending_audio_arm = None;
        self.notes.cancel();
        self.session.take()
    }

    pub fn advance_pass(&mut self, now: u64) {
        let Some(session) = self.session.as_mut().filter(|s| !s.audio) else {
            return;
        };
        let (Some(start), Some(length)) = (session.start, session.length_samples) else {
            return;
        };
        while now >= start.saturating_add((session.pass + 1).saturating_mul(length)) {
            let boundary = start + (session.pass + 1) * length;
            if let Some(take) = self.notes.next_pass(boundary) {
                session.working = apply_midi_take(
                    &session.working,
                    &take,
                    length as f64 / session.samples_per_beat(),
                );
            }
            session.pass += 1;
        }
    }

    pub fn note_input(&mut self, track_id: TrackId, pitch: u8, velocity: u8, on: bool, at: u64) {
        self.advance_pass(at);
        let Some(session) = self
            .session
            .as_ref()
            .filter(|s| !s.audio && s.working.track_id == track_id)
        else {
            return;
        };
        let Some(start) = session.start else {
            return;
        };
        if at >= start {
            self.notes.input_note(LoopRecordInput {
                target_id: Some(session.working.id),
                track_id,
                pitch,
                velocity,
                on,
                effective_at_samples: at,
                local_position_samples: Some(session.local_position(at)),
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn repeated_note(
        &mut self,
        track_id: TrackId,
        pitch: u8,
        velocity: u8,
        rate: NoteRepeatRate,
        at: u64,
        canonical_at: u64,
    ) {
        self.advance_pass(at);
        let Some(session) = self
            .session
            .as_ref()
            .filter(|s| !s.audio && s.working.track_id == track_id)
        else {
            return;
        };
        self.notes.repeated_note(
            Some(session.working.id),
            track_id,
            pitch,
            velocity,
            rate,
            at,
            Some(session.local_position(canonical_at)),
        );
    }

    pub fn preview(&mut self, now: u64) -> Option<LauncherClip> {
        self.advance_pass(now);
        let session = self
            .session
            .as_ref()
            .filter(|s| !s.audio && s.start.is_some() && s.stop.is_none())?;
        let spb = session.samples_per_beat();
        let local = session.local_position(now);
        let beats = session.length_samples.map_or_else(
            || free_recording_beats(now.saturating_sub(session.start.unwrap()), spb),
            |length| length as f64 / spb,
        );
        let notes = self.notes.note_counts();
        let replaced = if self.notes.mode == LoopRecordMode::Replace {
            session
                .working
                .timeline
                .get(session.working.track_id)
                .and_then(|content| content.note_clips.first())
                .map_or(0, |clip| {
                    clip.notes
                        .iter()
                        .filter(|note| note.start_beat < local as f64 / spb)
                        .count()
                })
        } else {
            0
        };
        let stamp = PreviewStamp {
            notes,
            pass: session.pass,
            length: (beats * spb).round() as u64,
            held_until: (notes.1 != 0).then_some(now),
            replaced,
        };
        if self.last_preview == Some(stamp) {
            return None;
        }
        let take = self.notes.snapshot(now, local)?;
        let clip = apply_midi_take(&session.working, &take, beats);
        self.last_preview = Some(stamp);
        Some(clip)
    }
}

impl PerformState {
    pub fn cancel_clip_record(&mut self, engine: &mut impl EngineHandle) -> Option<bool> {
        let armed = self.clip_record.notes.arm_was_sent();
        let session = self.clip_record.finish_session()?;
        engine.send(EngineCommand::StopClipRecord { immediate: true });
        if armed {
            self.clip_editor.next_request += 1;
            let request = self.clip_editor.next_request;
            self.clip_editor
                .queue_request(session.working.track_id, None, request);
            engine.send(EngineCommand::QueueClips {
                clips: vec![vibez_engine::playback_source::PreparedClipPlayback::stop(
                    session.working.track_id,
                    request,
                )],
                quantization: vibez_core::perform::MusicalBoundary::Immediate,
            });
        }
        let id = session.working.id;
        self.clip_editor.pending.retain(|_, clip| clip.id != id);
        let store = Arc::make_mut(&mut self.clips);
        store.clips.retain(|clip| clip.id != id);
        if let Some(original) = session.original {
            store.clips.push(original);
            self.select_launcher_clip(id);
        } else if self.clip_editor.selected == Some(id) {
            self.clip_editor.selected = None;
            self.clip_editor.editor = Default::default();
        }
        Some(session.audio)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::perform::{PerformCtx, PerformMsg};
    use crate::domains::test_support::RecordingEngine;

    fn recording(length: Option<u64>, mode: LoopRecordMode) -> ClipRecordState {
        let mut record = ClipRecordState::default();
        record.notes.mode = mode;
        let working = empty_midi_clip(ClipId::new(), TrackId::new(), 0, "Take".into(), 4.0);
        record.begin_session(
            ClipRecordSession {
                original: None,
                working,
                audio: false,
                length_samples: length,
                start: Some(0),
                output_start: Some(0),
                pass: 0,
                stop: None,
                sample_rate: 100,
                bpm: 60.0,
            },
            false,
        );
        let session = record.session.as_ref().unwrap();
        record
            .notes
            .start(session.working.id, session.working.track_id, 0, 0);
        record
    }

    #[test]
    fn unchanged_ticks_do_not_publish_but_held_notes_and_new_passes_do() {
        let mut record = recording(Some(400), LoopRecordMode::Overdub);
        let track = record.session.as_ref().unwrap().working.track_id;
        assert!(record.preview(0).is_some());
        for now in 1..100 {
            assert!(record.preview(now).is_none());
        }
        record.note_input(track, 60, 100, true, 100);
        assert!(record.preview(125).is_some());
        let held = record.preview(150).unwrap();
        assert_eq!(
            held.timeline.get(track).unwrap().note_clips[0].notes[0].duration_beats,
            0.5
        );
        record.note_input(track, 60, 0, false, 150);
        assert!(record.preview(150).is_some());
        assert!(record.preview(151).is_none());
        let second_pass = record.preview(400).unwrap();
        assert_eq!(
            second_pass.timeline.get(track).unwrap().note_clips[0]
                .notes
                .len(),
            1
        );
        assert!(record.preview(401).is_none());
    }

    #[test]
    fn replace_and_free_length_publish_when_the_audible_content_changes() {
        let mut record = recording(Some(400), LoopRecordMode::Replace);
        let session = record.session.as_mut().unwrap();
        let track = session.working.track_id;
        Arc::make_mut(&mut session.working.timeline)
            .ensure(track)
            .note_clips[0]
            .notes
            .push(MidiNote {
                pitch: 60,
                velocity: 100,
                start_beat: 1.0,
                duration_beats: 0.5,
            });
        assert!(record.preview(50).is_some());
        assert!(record.preview(99).is_none());
        let replaced = record.preview(101).unwrap();
        assert!(replaced.timeline.get(track).unwrap().note_clips[0]
            .notes
            .is_empty());
        assert!(record.preview(102).is_none());
        let mut free = recording(None, LoopRecordMode::Overdub);
        assert!(free.preview(1).is_some());
        assert!(free.preview(400).is_none());
        assert!(free.preview(402).is_some());
    }

    #[test]
    fn domain_owns_record_settings_and_start_stop_actions() {
        let mut state = PerformState {
            layout: vibez_project::PerformLayout::Clips,
            ..Default::default()
        };
        let mut engine = RecordingEngine::default();
        let ctx = PerformCtx::default();
        state.update(
            PerformMsg::ClipRecord(ClipRecordMsg::SetLength(RecordLength::Bars(2))),
            &mut engine,
            ctx,
        );
        assert_eq!(state.clip_record.length, RecordLength::Bars(2));
        let track = TrackId::new();
        let action = state.update(
            PerformMsg::ClipRecord(ClipRecordMsg::Slot(track, 3)),
            &mut engine,
            ctx,
        );
        assert_eq!(
            action.clip_record,
            Some(ClipRecordAction::Start {
                track_id: track,
                row: 3
            })
        );
        state.clip_record = recording(Some(400), LoopRecordMode::Overdub);
        let track = state.clip_record.session.as_ref().unwrap().working.track_id;
        state.update(
            PerformMsg::ClipRecord(ClipRecordMsg::SetMode(LoopRecordMode::Replace)),
            &mut engine,
            ctx,
        );
        assert_eq!(state.clip_record.notes.mode, LoopRecordMode::Overdub);
        let action = state.update(
            PerformMsg::ClipRecord(ClipRecordMsg::Slot(track, 0)),
            &mut engine,
            ctx,
        );
        assert_eq!(action.clip_record, Some(ClipRecordAction::Stop));
        assert_eq!(
            state
                .update(
                    PerformMsg::ClipRecord(ClipRecordMsg::Slot(track, 0)),
                    &mut engine,
                    ctx
                )
                .clip_record,
            None
        );
    }

    #[test]
    fn cancellation_during_count_in_clears_engine_and_restores_the_slot() {
        let mut state = PerformState {
            clip_record: recording(Some(400), LoopRecordMode::Replace),
            ..Default::default()
        };
        let session = state.clip_record.session.as_mut().unwrap();
        session.start = None;
        let original = session.working.clone();
        session.original = Some(original.clone());
        Arc::make_mut(&mut state.clips)
            .clips
            .push(session.working.clone());
        state.clip_editor.selected = Some(original.id);
        let mut engine = RecordingEngine::default();
        assert_eq!(state.cancel_clip_record(&mut engine), Some(false));
        assert!(matches!(
            engine.0[0],
            EngineCommand::StopClipRecord { immediate: true }
        ));
        assert!(!state.clip_record.is_active());
        assert!(Arc::ptr_eq(
            &state.clips.by_id(original.id).unwrap().timeline,
            &original.timeline
        ));
        assert_eq!(state.cancel_clip_record(&mut engine), None);
    }

    #[test]
    fn audio_arm_waits_for_the_first_captured_buffer() {
        let mut record = recording(Some(400), LoopRecordMode::Replace);
        let session = record.session.as_mut().unwrap();
        session.audio = true;
        record.pending_audio_arm = Some(session.working.prepare(1, 100.0));
        assert!(record.arm_audio_when_ready(None).is_none());
        assert!(matches!(
            record.arm_audio_when_ready(Some(128)),
            Some(EngineCommand::ArmClipRecord { .. })
        ));
        assert!(record.arm_audio_when_ready(Some(256)).is_none());
        let session = record.session.as_ref().unwrap();
        record.pending_audio_arm = Some(session.working.prepare(2, 100.0));
        record.finish_session();
        assert!(record.arm_audio_when_ready(Some(512)).is_none());
    }
}
