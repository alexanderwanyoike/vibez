//! Arrangement domain, tranche one: track lifecycle and mixing
//! basics. Owns the track list (the shared model other domains
//! receive explicitly), selection, and track numbering.
//!
//! Clip operations (moves, splits, warp orchestration) are the next
//! tranche; they follow this same pattern.

use vibez_core::id::TrackId;
use vibez_core::midi::TrackKind;
use vibez_engine::commands::EngineCommand;

use super::EngineHandle;
use crate::state::{ArrangementSelection, ArrangementState, UiTrack};

/// Messages the arrangement domain handles (track tranche).
#[derive(Debug, Clone)]
pub enum ArrangementMsg {
    AddTrack,
    AddMidiTrack,
    AddInstrumentTrack,
    RemoveTrack(TrackId),
    SelectTrack(TrackId),
    RenameTrack(TrackId, String),
    RenameClip(TrackId, vibez_core::id::ClipId, String),
    MoveTrackUp(TrackId),
    MoveTrackDown(TrackId),
    MoveSelectedTrackUp,
    MoveSelectedTrackDown,
    SetTrackGain(TrackId, f32),
    SetTrackPan(TrackId, f32),
    SetTrackMute(TrackId),
    SetTrackSolo(TrackId),
    EngineTrackMeter {
        track_id: TrackId,
        peak_l: f32,
        peak_r: f32,
    },
}

impl ArrangementMsg {
    /// Whether this message edits the project (drives the dirty flag).
    pub fn marks_dirty(&self) -> bool {
        !matches!(
            self,
            ArrangementMsg::SelectTrack(_) | ArrangementMsg::EngineTrackMeter { .. }
        )
    }
}

/// Cross-domain effects requested by an arrangement update.
#[derive(Debug, Default, PartialEq)]
pub struct ArrangementAction {
    /// All plugin GUI windows and raw pointers of this track must go
    /// (the track's devices are being destroyed).
    pub close_track_guis: Option<TrackId>,
    /// Status bar text.
    pub status: Option<String>,
}

impl ArrangementState {
    /// First track number with no name clash for the given prefix.
    pub fn next_unique_track_number(&mut self, prefix: &str) -> u32 {
        loop {
            let candidate = self.next_track_number;
            let name = format!("{prefix} {candidate}");
            if !self.tracks.iter().any(|t| t.name == name) {
                return candidate;
            }
            self.next_track_number += 1;
        }
    }

    fn find_track_mut(&mut self, track_id: TrackId) -> Option<&mut UiTrack> {
        self.tracks.iter_mut().find(|t| t.id == track_id)
    }

    fn move_track(&mut self, track_id: TrackId, up: bool, engine: &mut impl EngineHandle) {
        if let Some(idx) = self.tracks.iter().position(|t| t.id == track_id) {
            let target = if up {
                idx.checked_sub(1)
            } else if idx + 1 < self.tracks.len() {
                Some(idx + 1)
            } else {
                None
            };
            if let Some(target) = target {
                self.tracks.swap(idx, target);
                let order: Vec<TrackId> = self.tracks.iter().map(|t| t.id).collect();
                engine.send(EngineCommand::ReorderTracks(order));
            }
        }
    }

    pub fn update(
        &mut self,
        msg: ArrangementMsg,
        engine: &mut impl EngineHandle,
    ) -> ArrangementAction {
        let mut action = ArrangementAction::default();
        match msg {
            ArrangementMsg::AddTrack => {
                let track_num = self.next_unique_track_number("Track");
                let color_index = (track_num.wrapping_sub(1) % 8) as u8;
                self.next_track_number = track_num + 1;
                let id = TrackId::new();
                let name = format!("Track {track_num}");
                engine.send(EngineCommand::AddTrack(id, name.clone()));
                self.tracks.push(UiTrack::new(id, name, color_index));
                self.selected_track = Some(id);
                action.status = Some(format!("{} tracks", self.tracks.len()));
            }
            ArrangementMsg::AddMidiTrack | ArrangementMsg::AddInstrumentTrack => {
                let track_num = self.next_unique_track_number("MIDI");
                let color_index = (track_num.wrapping_sub(1) % 8) as u8;
                self.next_track_number = track_num + 1;
                let id = TrackId::new();
                let name = format!("MIDI {track_num}");
                engine.send(EngineCommand::AddMidiTrack(id, name.clone()));
                let mut track = UiTrack::new_instrument(id, name, TrackKind::Midi, color_index);
                track.has_instrument = false;
                self.tracks.push(track);
                self.selected_track = Some(id);
                action.status = Some(format!("{} tracks", self.tracks.len()));
            }
            ArrangementMsg::RemoveTrack(track_id) => {
                let removed_name = self
                    .tracks
                    .iter()
                    .find(|t| t.id == track_id)
                    .map(|t| t.name.clone())
                    .unwrap_or_else(|| format!("{track_id}"));

                engine.send(EngineCommand::RemoveTrack(track_id));
                self.tracks.retain(|t| t.id != track_id);
                if self.selected_track == Some(track_id) {
                    self.selected_track = self.tracks.first().map(|t| t.id);
                }
                if let Some((tid, _)) = self.selected_note_clip {
                    if tid == track_id {
                        self.selected_note_clip = None;
                    }
                }
                self.selected_clips.retain(|sel| {
                    let sel_track = match sel {
                        ArrangementSelection::AudioClip { track_id: t, .. } => *t,
                        ArrangementSelection::NoteClip { track_id: t, .. } => *t,
                    };
                    sel_track != track_id
                });
                action.close_track_guis = Some(track_id);
                action.status = Some(format!(
                    "Removed {removed_name}. {} track(s) remain.",
                    self.tracks.len()
                ));
            }
            ArrangementMsg::SelectTrack(track_id) => {
                self.selected_track = Some(track_id);
            }
            ArrangementMsg::RenameTrack(track_id, new_name) => {
                if let Some(track) = self.find_track_mut(track_id) {
                    track.name = new_name;
                }
            }
            ArrangementMsg::RenameClip(track_id, clip_id, new_name) => {
                if let Some(track) = self.find_track_mut(track_id) {
                    if let Some(c) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                        c.name = new_name.clone();
                    }
                    if let Some(c) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        c.name = new_name;
                    }
                }
            }
            ArrangementMsg::MoveTrackUp(track_id) => self.move_track(track_id, true, engine),
            ArrangementMsg::MoveTrackDown(track_id) => self.move_track(track_id, false, engine),
            ArrangementMsg::MoveSelectedTrackUp => {
                if let Some(tid) = self.selected_track {
                    self.move_track(tid, true, engine);
                }
            }
            ArrangementMsg::MoveSelectedTrackDown => {
                if let Some(tid) = self.selected_track {
                    self.move_track(tid, false, engine);
                }
            }
            ArrangementMsg::SetTrackGain(track_id, gain) => {
                let gain = gain.clamp(0.0, 2.0);
                engine.send(EngineCommand::SetTrackGain(track_id, gain));
                if let Some(track) = self.find_track_mut(track_id) {
                    track.gain = gain;
                }
            }
            ArrangementMsg::SetTrackPan(track_id, pan) => {
                let pan = pan.clamp(0.0, 1.0);
                engine.send(EngineCommand::SetTrackPan(track_id, pan));
                if let Some(track) = self.find_track_mut(track_id) {
                    track.pan = pan;
                }
            }
            ArrangementMsg::SetTrackMute(track_id) => {
                if let Some(track) = self.find_track_mut(track_id) {
                    track.mute = !track.mute;
                    let mute = track.mute;
                    engine.send(EngineCommand::SetTrackMute(track_id, mute));
                }
            }
            ArrangementMsg::SetTrackSolo(track_id) => {
                if let Some(track) = self.find_track_mut(track_id) {
                    track.solo = !track.solo;
                    let solo = track.solo;
                    engine.send(EngineCommand::SetTrackSolo(track_id, solo));
                }
            }
            ArrangementMsg::EngineTrackMeter {
                track_id,
                peak_l,
                peak_r,
            } => {
                if let Some(track) = self.find_track_mut(track_id) {
                    track.peak_l = peak_l.max(track.peak_l * 0.85);
                    track.peak_r = peak_r.max(track.peak_r * 0.85);
                }
            }
        }
        action
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::RecordingEngine;
    use super::*;

    fn arrangement_with_tracks(n: usize) -> ArrangementState {
        let mut a = ArrangementState {
            next_track_number: 1,
            ..Default::default()
        };
        let mut engine = RecordingEngine::default();
        for _ in 0..n {
            a.update(ArrangementMsg::AddTrack, &mut engine);
        }
        a
    }

    #[test]
    fn add_track_selects_it_and_names_uniquely() {
        let a = arrangement_with_tracks(2);
        assert_eq!(a.tracks.len(), 2);
        assert_eq!(a.tracks[1].name, "Track 2");
        assert_eq!(a.selected_track, Some(a.tracks[1].id));
    }

    #[test]
    fn remove_track_clears_its_selections_and_requests_gui_teardown() {
        let mut a = arrangement_with_tracks(2);
        let victim = a.tracks[1].id;
        let survivor = a.tracks[0].id;
        a.selected_note_clip = Some((victim, vibez_core::id::ClipId::new()));
        let mut engine = RecordingEngine::default();
        let action = a.update(ArrangementMsg::RemoveTrack(victim), &mut engine);
        assert_eq!(a.tracks.len(), 1);
        assert_eq!(a.selected_track, Some(survivor));
        assert_eq!(a.selected_note_clip, None);
        assert_eq!(action.close_track_guis, Some(victim));
    }

    #[test]
    fn reorder_sends_full_order_and_respects_bounds() {
        let mut a = arrangement_with_tracks(2);
        let first = a.tracks[0].id;
        let mut engine = RecordingEngine::default();
        // Top track cannot move further up: no command.
        a.update(ArrangementMsg::MoveTrackUp(first), &mut engine);
        assert!(engine.0.is_empty());
        a.update(ArrangementMsg::MoveTrackDown(first), &mut engine);
        assert_eq!(a.tracks[1].id, first);
        assert!(matches!(engine.0[0], EngineCommand::ReorderTracks(_)));
    }

    #[test]
    fn gain_and_pan_clamp() {
        let mut a = arrangement_with_tracks(1);
        let id = a.tracks[0].id;
        let mut engine = RecordingEngine::default();
        a.update(ArrangementMsg::SetTrackGain(id, 99.0), &mut engine);
        a.update(ArrangementMsg::SetTrackPan(id, -5.0), &mut engine);
        assert_eq!(a.tracks[0].gain, 2.0);
        assert_eq!(a.tracks[0].pan, 0.0);
    }

    #[test]
    fn meter_decays_instead_of_snapping() {
        let mut a = arrangement_with_tracks(1);
        let id = a.tracks[0].id;
        a.tracks[0].peak_l = 1.0;
        let mut engine = RecordingEngine::default();
        a.update(
            ArrangementMsg::EngineTrackMeter {
                track_id: id,
                peak_l: 0.0,
                peak_r: 0.0,
            },
            &mut engine,
        );
        assert!((a.tracks[0].peak_l - 0.85).abs() < 1e-6);
    }
}
