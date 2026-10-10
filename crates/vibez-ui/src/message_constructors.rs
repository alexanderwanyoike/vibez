//! Existing message constructors, separated from message payloads.

use super::*;

/// Arity-preserving constructor helpers so call sites read cleanly
/// and mechanical migrations stay parenthesis-balanced.
impl Message {
    pub fn in_undo_gesture(self, id: UndoGestureId) -> Self {
        Self::UndoGesture {
            id,
            edit: Box::new(self),
        }
    }

    pub fn add_effect(t: TrackId, e: EffectType) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::AddEffect(t, e))
    }
    pub fn remove_effect(t: TrackId, e: EffectId) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::RemoveEffect(t, e))
    }
    pub fn set_effect_param(t: TrackId, e: EffectId, i: usize, v: f32) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::SetEffectParam(
            t, e, i, v,
        ))
    }
    pub fn set_effect_params(t: TrackId, e: EffectId, updates: Vec<(usize, f32)>) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::SetEffectParams(
            t, e, updates,
        ))
    }
    pub fn toggle_effect_bypass(t: TrackId, e: EffectId) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::ToggleEffectBypass(
            t, e,
        ))
    }
    pub fn move_effect_up(t: TrackId, e: EffectId) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::MoveEffectUp(t, e))
    }
    pub fn move_effect_down(t: TrackId, e: EffectId) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::MoveEffectDown(t, e))
    }
    pub fn set_track_instrument(t: TrackId, k: InstrumentKind) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::SetTrackInstrument(
            t, k,
        ))
    }
    pub fn remove_track_instrument(t: TrackId) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::RemoveTrackInstrument(
            t,
        ))
    }
    pub fn set_instrument_param(t: TrackId, i: usize, v: f32) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::SetInstrumentParam(
            t, i, v,
        ))
    }
    pub fn select_drum_rack_pad(t: TrackId, p: usize) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::SelectDrumRackPad(t, p))
    }
    pub fn clear_drum_rack_pad(t: TrackId, p: usize) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::ClearDrumRackPad(t, p))
    }
    pub fn audition_note(track_id: TrackId, pitch: u8, on: bool) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::AuditionNote {
            track_id,
            pitch,
            on,
        })
    }
    pub fn dismiss_device_menu() -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::DismissContextMenu)
    }
    pub fn set_device_menu_category(c: crate::state::DeviceMenuCategory) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::SetMenuCategory(c))
    }
    pub fn device_menu_search(q: String) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::MenuSearch(q))
    }
}

impl Message {
    pub fn menu_item(overlay: MenuOverlay, action: Self) -> Self {
        Self::MenuItemSelected(overlay, Box::new(action))
    }

    pub fn dismiss_menu(overlay: MenuOverlay) -> Self {
        Self::DismissMenu(overlay)
    }

    pub fn select_track(t: TrackId) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::SelectTrack(t))
    }
    pub fn remove_track(t: TrackId) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::RequestRemoveTrack(t))
    }
    pub fn rename_track(t: TrackId, n: String) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::RenameTrack(
            t, n,
        ))
    }
    pub fn rename_clip(t: TrackId, c: ClipId, n: String) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::RenameClip(
            t, c, n,
        ))
    }
    pub fn move_track_up(t: TrackId) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::MoveTrackUp(t))
    }
    pub fn move_track_down(t: TrackId) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::MoveTrackDown(
            t,
        ))
    }
    pub fn set_track_gain(t: TrackId, g: f32) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::SetTrackGain(
            t, g,
        ))
    }
    pub fn set_track_pan(t: TrackId, p: f32) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::SetTrackPan(
            t, p,
        ))
    }
    pub fn set_track_mute(t: TrackId) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::SetTrackMute(t))
    }
    pub fn set_track_solo(t: TrackId) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::SetTrackSolo(t))
    }
    pub fn add_bus() -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::AddBus)
    }
    pub fn remove_bus(bus: TrackId) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::RemoveBus(bus))
    }
    pub fn set_send(t: TrackId, bus: TrackId, amount: f32) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::SetSend {
            track_id: t,
            bus_id: bus,
            amount,
        })
    }
}

impl Message {
    pub fn remove_clip(t: TrackId, c: ClipId) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::RemoveClip(
            t, c,
        ))
    }
    pub fn toggle_clip_loop(t: TrackId, c: ClipId) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::ToggleClipLoop(
            t, c,
        ))
    }
    pub fn set_time_selection_active(a: bool) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::SetTimeSelectionActive(a))
    }
    pub fn duplicate_note_clip(t: TrackId, c: ClipId) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::DuplicateNoteClip(t, c))
    }
    pub fn split_audio_clip(t: TrackId, c: ClipId, split_position: u64) -> Self {
        Self::Arrangement(
            crate::domains::arrangement::ArrangementMsg::SplitAudioClip {
                track_id: t,
                clip_id: c,
                split_position,
            },
        )
    }
    pub fn split_note_clip(t: TrackId, c: ClipId, split_beat: f64) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::SplitNoteClip {
            track_id: t,
            clip_id: c,
            split_beat,
        })
    }
    pub fn split_selected_at_playhead() -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::SplitSelectedAtPlayhead)
    }
    pub fn join_selected_clips() -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::JoinSelectedClips)
    }
    pub fn delete_clips_in_region(
        start_beats: f64,
        end_beats: f64,
        track_id: Option<TrackId>,
    ) -> Self {
        Self::Arrangement(
            crate::domains::arrangement::ArrangementMsg::DeleteClipsInRegion {
                start_beats,
                end_beats,
                track_id,
            },
        )
    }
    pub fn split_clips_at_region(
        start_beats: f64,
        end_beats: f64,
        track_id: Option<TrackId>,
    ) -> Self {
        Self::Arrangement(
            crate::domains::arrangement::ArrangementMsg::SplitClipsAtRegion {
                start_beats,
                end_beats,
                track_id,
            },
        )
    }
    pub fn create_clip_from_selection() -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::CreateClipFromSelection)
    }
    pub fn create_note_clip_from_selection(t: TrackId) -> Self {
        Self::Arrangement(
            crate::domains::arrangement::ArrangementMsg::CreateNoteClipFromSelection(t),
        )
    }
}
