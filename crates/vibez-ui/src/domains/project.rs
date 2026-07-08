//! Project domain, tranche one: snapshot plumbing for undo/redo.
//!
//! Plugin device instances live on the engine side and cannot be
//! cloned into [`crate::state::ProjectSnapshot`]s, so restoring a
//! snapshot has to reload them through the same background pipeline
//! project open uses. This module owns the pure part of that:
//! deciding which devices to reload, at which chain positions, and
//! with which state blob.

use vibez_core::effect::PluginDeviceInfo;
use vibez_core::id::{EffectId, TrackId};
use vibez_plugin_host::gui::PluginGuiKey;

use crate::state::{ProjectSnapshot, ProjectState};

/// Messages the project domain handles (file menu + undo history).
#[derive(Debug, Clone)]
pub enum ProjectMsg {
    ToggleFileMenu,
    DismissFileMenu,
    Undo,
    Redo,
}

/// Read-only cross-domain facts for project updates.
#[derive(Debug)]
pub struct ProjectCtx {
    /// A snapshot of the CURRENT editable state, taken by the router
    /// just before this update; undo/redo push it onto the opposite
    /// stack.
    pub snapshot_now: ProjectSnapshot,
}

/// Cross-domain effects requested by a project update.
#[derive(Debug, Default)]
pub struct ProjectAction {
    /// Status bar text.
    pub status: Option<String>,
    /// Restore this snapshot (tears down and replays the engine).
    pub apply_snapshot: Option<ProjectSnapshot>,
}

impl ProjectState {
    pub fn update(&mut self, msg: ProjectMsg, ctx: ProjectCtx) -> ProjectAction {
        let mut action = ProjectAction::default();
        match msg {
            ProjectMsg::ToggleFileMenu => {
                self.file_menu_open = !self.file_menu_open;
            }
            ProjectMsg::DismissFileMenu => {
                self.file_menu_open = false;
            }
            ProjectMsg::Undo => {
                let Some(snapshot) = self.history.pop_undo() else {
                    action.status = Some("Nothing to undo".to_string());
                    return action;
                };
                self.history.push_redo(ctx.snapshot_now);
                action.apply_snapshot = Some(snapshot);
                action.status = Some("Undo".to_string());
            }
            ProjectMsg::Redo => {
                let Some(snapshot) = self.history.pop_redo() else {
                    action.status = Some("Nothing to redo".to_string());
                    return action;
                };
                self.history.push_undo(ctx.snapshot_now);
                action.apply_snapshot = Some(snapshot);
                action.status = Some("Redo".to_string());
            }
        }
        action
    }
}

/// Plugin reload work orders extracted from a snapshot.
#[derive(Debug, Default)]
pub struct PluginReloadRequests {
    /// (track, effect slot, chain position, device) per plugin effect.
    pub effects: Vec<(TrackId, EffectId, usize, PluginDeviceInfo)>,
    /// (track, device) per plugin instrument.
    pub instruments: Vec<(TrackId, PluginDeviceInfo)>,
}

/// Strip plugin devices out of a snapshot's tracks and return reload
/// requests for them.
///
/// `capture_state` is called for devices that still have a live
/// instance so undo restores their exact current state instead of the
/// (possibly stale, possibly absent) blob recorded at project load.
/// Plugin effect entries are removed from the snapshot's chains; the
/// load pipeline re-inserts them at `chain position` when the reload
/// completes, exactly like project open. Plugin instrument fields
/// stay on the track (the card renders immediately; the arriving
/// instance overwrites them idempotently).
pub fn collect_plugin_reload_requests(
    snapshot: &mut ProjectSnapshot,
    mut capture_state: impl FnMut(PluginGuiKey) -> Option<String>,
) -> PluginReloadRequests {
    let mut requests = PluginReloadRequests::default();
    let (tracks, master) = (&mut snapshot.tracks, &mut snapshot.master);
    for track in tracks.iter_mut().chain(std::iter::once(master)) {
        let track_id = track.id;
        for (chain_pos, effect) in track.effects.iter().enumerate() {
            if let Some(dev) = &effect.plugin_ref {
                let mut dev = dev.clone();
                if let Some(state) = capture_state(PluginGuiKey::Effect {
                    track_id,
                    effect_id: effect.id,
                }) {
                    dev.state_b64 = Some(state);
                }
                requests.effects.push((track_id, effect.id, chain_pos, dev));
            }
        }
        track.effects.retain(|e| e.plugin_ref.is_none());
        if let Some(dev) = &track.plugin_instrument_ref {
            let mut dev = dev.clone();
            if let Some(state) = capture_state(PluginGuiKey::Instrument { track_id }) {
                dev.state_b64 = Some(state);
            }
            requests.instruments.push((track_id, dev));
        }
    }
    requests
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{UiEffect, UiTrack};
    use std::collections::HashSet;
    use vibez_core::effect::EffectType;

    fn plugin_device(name: &str) -> PluginDeviceInfo {
        PluginDeviceInfo {
            format: "vst3".to_string(),
            uid: format!("uid-{name}"),
            path: format!("/plugins/{name}.vst3").into(),
            name: name.to_string(),
            state_b64: Some("stale".to_string()),
        }
    }

    fn effect(plugin: Option<PluginDeviceInfo>) -> UiEffect {
        UiEffect {
            id: EffectId::new(),
            effect_type: EffectType::Gain,
            bypass: false,
            params: Vec::new(),
            descriptors: &[],
            plugin_name: plugin.as_ref().map(|d| d.name.clone()),
            has_plugin_gui: false,
            plugin_ref: plugin,
        }
    }

    fn snapshot_with(effects: Vec<UiEffect>) -> ProjectSnapshot {
        let mut track = UiTrack::new(TrackId::new(), "T1".to_string(), 0);
        track.effects = effects;
        ProjectSnapshot {
            tracks: vec![track],
            master: crate::state::new_master_track(),
            bpm: 120.0,
            bpm_text: "120".to_string(),
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
            selected_track: None,
            selected_clips: HashSet::new(),
            selected_note_clip: None,
            next_track_number: 2,
        }
    }

    #[test]
    fn plugin_effects_become_requests_at_their_chain_position() {
        let mut snap = snapshot_with(vec![
            effect(None),
            effect(Some(plugin_device("comp"))),
            effect(None),
        ]);
        let requests = collect_plugin_reload_requests(&mut snap, |_| None);
        assert_eq!(requests.effects.len(), 1);
        let (_, _, chain_pos, dev) = &requests.effects[0];
        assert_eq!(*chain_pos, 1);
        assert_eq!(dev.name, "comp");
        // Plugin slot stripped; builtins remain for direct replay.
        assert_eq!(snap.tracks[0].effects.len(), 2);
        assert!(snap.tracks[0]
            .effects
            .iter()
            .all(|e| e.plugin_ref.is_none()));
    }

    #[test]
    fn live_state_capture_overrides_stale_blob() {
        let mut snap = snapshot_with(vec![effect(Some(plugin_device("eq")))]);
        let requests = collect_plugin_reload_requests(&mut snap, |_| Some("fresh".to_string()));
        assert_eq!(requests.effects[0].3.state_b64.as_deref(), Some("fresh"));
    }

    #[test]
    fn dead_instance_keeps_recorded_blob() {
        let mut snap = snapshot_with(vec![effect(Some(plugin_device("eq")))]);
        let requests = collect_plugin_reload_requests(&mut snap, |_| None);
        assert_eq!(requests.effects[0].3.state_b64.as_deref(), Some("stale"));
    }

    fn ctx() -> ProjectCtx {
        ProjectCtx {
            snapshot_now: snapshot_with(Vec::new()),
        }
    }

    #[test]
    fn undo_pops_history_and_stashes_current_state_for_redo() {
        let mut p = ProjectState::default();
        p.history.push_undo(snapshot_with(Vec::new()));
        let action = p.update(ProjectMsg::Undo, ctx());
        assert!(action.apply_snapshot.is_some());
        assert_eq!(action.status.as_deref(), Some("Undo"));
        assert_eq!(p.history.undo.len(), 0);
        assert_eq!(p.history.redo.len(), 1);
    }

    #[test]
    fn undo_with_empty_history_reports_and_applies_nothing() {
        let mut p = ProjectState::default();
        let action = p.update(ProjectMsg::Undo, ctx());
        assert!(action.apply_snapshot.is_none());
        assert_eq!(action.status.as_deref(), Some("Nothing to undo"));
        assert_eq!(p.history.redo.len(), 0);
    }

    #[test]
    fn plugin_instrument_is_requested_and_fields_kept() {
        let mut snap = snapshot_with(Vec::new());
        snap.tracks[0].plugin_instrument_ref = Some(plugin_device("synth"));
        snap.tracks[0].has_instrument = true;
        let requests = collect_plugin_reload_requests(&mut snap, |_| None);
        assert_eq!(requests.instruments.len(), 1);
        assert!(snap.tracks[0].plugin_instrument_ref.is_some());
        assert!(snap.tracks[0].has_instrument);
    }
}
