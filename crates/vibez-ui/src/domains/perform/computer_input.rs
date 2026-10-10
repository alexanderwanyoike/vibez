//! A physical press resolves its mode and remembers the original release owner.

use super::*;

impl PerformState {
    pub(super) fn computer_key_pressed(
        &mut self,
        key: ComputerKey,
        key_id: String,
        occurred_at: Instant,
        engine: &mut impl EngineHandle,
        ctx: PerformCtx<'_>,
    ) -> PerformAction {
        if let Some(position) = self.key_rebind_target.take() {
            self.input_mapping.rebind(position, key);
            return PerformAction {
                keyboard_consumed: true,
                persist_settings: true,
                gesture: None,
                ..PerformAction::default()
            };
        }
        if !ctx.workspace_visible {
            return PerformAction::default();
        }
        if self.active_computer_keys.contains_key(&key_id) {
            return PerformAction {
                keyboard_consumed: true,
                ..PerformAction::default()
            };
        }
        let Some(position) = self.input_mapping.position_for(key) else {
            return PerformAction::default();
        };
        let source = PadGestureSource::ComputerKeyboard { key };
        let selected_instrument_target = (self.mode == PerformMode::Instrument
            && self.instrument_target_overlay)
            .then(|| {
                self.track_for_instrument_target_pad(position, ctx.project_tracks)
                    .map(|track| track.id)
            })
            .flatten();
        if let Some(track_id) = selected_instrument_target {
            self.sync_instrument_target_from_selection(Some(track_id), ctx.project_tracks);
        }
        let instrument_note =
            if self.mode == PerformMode::Instrument && !self.instrument_target_overlay {
                let velocity = self.fixed_computer_velocity();
                self.live_instrument_target(ctx.workspace_visible, ctx.project_tracks)
                    .map(|track_id| {
                        let mut note = self.resolve_instrument_note(position, velocity, track_id);
                        note.repeating = self.note_repeat_active();
                        note
                    })
            } else {
                None
            };
        if let Some(note) = instrument_note {
            if note.repeating {
                self.start_note_repeat(note, engine);
            } else {
                engine.send(vibez_engine::commands::EngineCommand::ExternalNoteOn {
                    track_id: note.track_id,
                    pitch: note.pitch,
                    velocity: note.velocity,
                });
            }
        }
        self.active_computer_keys
            .insert(key_id, (position, source, instrument_note));
        let track_mute_request = (self.mode == PerformMode::TrackMutes)
            .then(|| self.track_mute_request(position, ctx.project_tracks))
            .flatten();
        let section_launch = if self.layout == vibez_project::PerformLayout::Sections
            && self.mode == PerformMode::Sections
            && !self.section_record.is_active()
        {
            let slot = u16::from(self.banks.sections) * 16 + position.index() as u16;
            self.sections.at_slot(slot).map(|section| section.id)
        } else {
            None
        };
        if let Some(section_id) = section_launch {
            self.select_section(section_id, ctx.selected_project_track);
        }
        PerformAction {
            keyboard_consumed: true,
            focus_clip_tab: false,
            persist_settings: false,
            gesture: Some(PadGesture {
                position,
                kind: PadGestureKind::Press,
                velocity: None,
                source,
                occurred_at,
            }),
            track_mute_request,
            track_swing_request: None,
            select_project_track: selected_instrument_target,
            clip_launch: if self.layout == vibez_project::PerformLayout::Clips
                && self.mode == PerformMode::Sections
            {
                ctx.project_tracks
                    .get(self.clip_editor.first_track + position.column as usize)
                    .and_then(|track| {
                        self.clips
                            .at(
                                track.id,
                                self.clip_editor.first_row + u32::from(position.row),
                            )
                            .map_or(
                                Some(clip_launcher::ClipLaunchRequest::Stop(track.id)),
                                |clip| self.toggle_clip_request(clip.id),
                            )
                    })
            } else {
                None
            },
            section_launch,
            section_content_changed: None,
            capture: None,
            section_record: None,
            clip_record: None,
            section_record_status: None,
        }
    }
}
