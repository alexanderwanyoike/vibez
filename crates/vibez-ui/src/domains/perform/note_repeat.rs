//! Instrument Note Repeat and Swing interaction behavior.

use super::*;

impl PerformState {
    pub(super) fn update_project_swing(&mut self, value: f32, engine: &mut impl EngineHandle) {
        self.project_swing = SwingAmount::new(value);
        engine.send(vibez_engine::commands::EngineCommand::SetProjectSwing(
            self.project_swing,
        ));
    }

    pub(super) fn update_track_swing(
        &self,
        value: Option<f32>,
        engine: &mut impl EngineHandle,
        ctx: PerformCtx<'_>,
    ) -> PerformAction {
        let Some(track_id) = self
            .instrument_target()
            .filter(|track_id| ctx.project_tracks.iter().any(|track| track.id == *track_id))
        else {
            return PerformAction::default();
        };
        let swing_offset = value.map(SwingOffset::new);
        engine.send(vibez_engine::commands::EngineCommand::SetTrackSwingOffset(
            track_id,
            swing_offset,
        ));
        PerformAction {
            track_swing_request: Some(TrackSwingRequest {
                track_id,
                swing_offset,
            }),
            ..PerformAction::default()
        }
    }

    pub(super) fn update_note_repeat_rate(
        &mut self,
        rate: NoteRepeatRate,
        engine: &mut impl EngineHandle,
    ) {
        self.note_repeat_rate = rate;
        if !self.note_repeat_active() {
            return;
        }
        for (_, _, note) in self.active_computer_keys.values() {
            let Some(note) = note.filter(|note| note.repeating) else {
                continue;
            };
            engine.send(
                vibez_engine::commands::EngineCommand::UpdateNoteRepeatRate {
                    id: note.repeat_id,
                    track_id: note.track_id,
                    rate,
                },
            );
        }
    }

    pub(super) fn update_note_repeat_momentary(
        &mut self,
        active: bool,
        key_id: Option<String>,
        engine: &mut impl EngineHandle,
    ) -> PerformAction {
        let was_active = self.note_repeat_active();
        self.note_repeat_momentary = active;
        self.note_repeat_momentary_key_id = active.then_some(key_id).flatten();
        self.sync_note_repeat_activation(was_active, engine);
        PerformAction {
            keyboard_consumed: true,
            ..PerformAction::default()
        }
    }

    pub(super) fn toggle_note_repeat_latch(&mut self, engine: &mut impl EngineHandle) {
        let was_active = self.note_repeat_active();
        self.note_repeat_latched = !self.note_repeat_latched;
        self.sync_note_repeat_activation(was_active, engine);
    }

    pub(super) fn start_note_repeat(
        &self,
        note: ActiveInstrumentNote,
        engine: &mut impl EngineHandle,
    ) {
        engine.send(vibez_engine::commands::EngineCommand::StartNoteRepeat {
            id: note.repeat_id,
            track_id: note.track_id,
            pitch: note.pitch,
            velocity: note.velocity,
            rate: self.note_repeat_rate,
        });
    }

    pub(super) fn stop_note_repeat(note: ActiveInstrumentNote, engine: &mut impl EngineHandle) {
        engine.send(vibez_engine::commands::EngineCommand::StopNoteRepeat {
            id: note.repeat_id,
            track_id: note.track_id,
        });
    }

    pub const fn project_swing(&self) -> SwingAmount {
        self.project_swing
    }

    pub const fn note_repeat_rate(&self) -> NoteRepeatRate {
        self.note_repeat_rate
    }

    pub const fn note_repeat_latched(&self) -> bool {
        self.note_repeat_latched
    }

    pub const fn note_repeat_active(&self) -> bool {
        self.note_repeat_momentary || self.note_repeat_latched
    }

    pub fn note_repeat_momentary_key_id(&self) -> Option<&str> {
        self.note_repeat_momentary_key_id.as_deref()
    }

    pub fn set_project_swing(&mut self, swing: SwingAmount) {
        self.project_swing = swing;
    }

    fn sync_note_repeat_activation(&mut self, was_active: bool, engine: &mut impl EngineHandle) {
        let active = self.note_repeat_active();
        if active == was_active {
            return;
        }
        let rate = self.note_repeat_rate;
        for (_, _, note) in self.active_computer_keys.values_mut() {
            let Some(note) = note else {
                continue;
            };
            if active {
                engine.send(vibez_engine::commands::EngineCommand::StartNoteRepeat {
                    id: note.repeat_id,
                    track_id: note.track_id,
                    pitch: note.pitch,
                    velocity: note.velocity,
                    rate,
                });
            } else {
                Self::stop_note_repeat(*note, engine);
            }
            note.repeating = active;
        }
    }
}
