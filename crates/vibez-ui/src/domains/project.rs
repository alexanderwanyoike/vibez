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
    ToggleRecentProjects,
    ClearRecentProjects,
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
    /// Global recent-project history changed and must be saved to UI settings.
    pub persist_ui_settings: bool,
}

impl ProjectState {
    pub fn update(&mut self, msg: ProjectMsg, ctx: ProjectCtx) -> ProjectAction {
        let mut action = ProjectAction::default();
        if self.history.transaction_active() && matches!(&msg, ProjectMsg::Undo | ProjectMsg::Redo)
        {
            action.status = Some("Finish the current project transaction first".to_string());
            return action;
        }
        match msg {
            ProjectMsg::ToggleFileMenu => {
                self.file_menu_open = !self.file_menu_open;
                if !self.file_menu_open {
                    self.recent_projects_open = false;
                }
            }
            ProjectMsg::ToggleRecentProjects => {
                if self.file_menu_open {
                    self.recent_projects_open = !self.recent_projects_open;
                }
            }
            ProjectMsg::ClearRecentProjects => {
                self.recent_project_paths.clear();
                self.recent_projects_open = false;
                action.persist_ui_settings = true;
                action.status = Some("Cleared recent projects".to_string());
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
// A reload may outlive slot deletion, replacement or project reset. Its saved
// identity must still belong to the current project before phase-two init.
pub fn accepts_effect_load(
    track: Option<&crate::state::ProjectTrack>,
    effect_id: vibez_core::id::EffectId,
    restoring_slot: bool,
    device: &vibez_core::effect::PluginDeviceInfo,
) -> bool {
    track.is_some_and(|track| {
        if restoring_slot {
            track
                .effects
                .iter()
                .any(|slot| slot.id == effect_id && slot.plugin_ref.as_ref() == Some(device))
        } else {
            track.has_device_room()
        }
    })
}

/// Records the live state of every plugin device in `snapshot`. A plugin
/// instance cannot live in a snapshot, so restoring one reloads it from the
/// state its device reference carries; recording the live state there keeps
/// the sound the plugin has now rather than the one it loaded with, even
/// after the instance is gone.
pub fn record_live_plugin_states(
    snapshot: &mut ProjectSnapshot,
    mut capture_state: impl FnMut(PluginGuiKey) -> Option<String>,
) {
    let has_plugins = snapshot.project_tracks.channels().any(|track| {
        track.plugin_instrument_ref.is_some()
            || track
                .effects
                .iter()
                .any(|effect| effect.plugin_ref.is_some())
    });
    if !has_plugins {
        return;
    }
    let project_tracks = std::sync::Arc::make_mut(&mut snapshot.project_tracks);
    for track in project_tracks
        .tracks
        .iter_mut()
        .chain(std::iter::once(&mut project_tracks.master))
        .chain(project_tracks.buses.iter_mut())
    {
        let track_id = track.id;
        for effect in &mut track.effects {
            let key = PluginGuiKey::Effect {
                track_id,
                effect_id: effect.id,
            };
            if let Some(device) = effect.plugin_ref.as_mut() {
                keep_live_state(device, capture_state(key));
            }
        }
        if let Some(device) = track.plugin_instrument_ref.as_mut() {
            keep_live_state(device, capture_state(PluginGuiKey::Instrument { track_id }));
        }
    }
}

/// `live` replaces the state `device` was recorded with; without a live
/// instance the recorded state stays.
pub fn keep_live_state(device: &mut PluginDeviceInfo, live: Option<String>) {
    if let Some(state) = live {
        device.state_b64 = Some(state);
    }
}

/// Records the live state of `track_id`'s plugin instrument in `snapshot`,
/// the undo step taken just before an edit that removes the instance, so
/// undoing that edit restores the sound the plugin had rather than the one
/// it loaded with.
pub fn record_plugin_instrument_state(
    snapshot: &mut ProjectSnapshot,
    track_id: TrackId,
    capture_state: impl FnOnce(PluginGuiKey) -> Option<String>,
) {
    let has_plugin = snapshot
        .project_tracks
        .find(track_id)
        .is_some_and(|track| track.plugin_instrument_ref.is_some());
    if !has_plugin {
        return;
    }
    let Some(state) = capture_state(PluginGuiKey::Instrument { track_id }) else {
        return;
    };
    if let Some(device) = std::sync::Arc::make_mut(&mut snapshot.project_tracks)
        .find_mut(track_id)
        .and_then(|track| track.plugin_instrument_ref.as_mut())
    {
        device.state_b64 = Some(state);
    }
}

/// Retaining unavailable slots preserves sidechain assignments and chain identity
/// while plugin reloads complete asynchronously.
pub fn collect_plugin_reload_requests(
    snapshot: &mut ProjectSnapshot,
    mut capture_state: impl FnMut(PluginGuiKey) -> Option<String>,
) -> PluginReloadRequests {
    let mut requests = PluginReloadRequests::default();
    let project_tracks = std::sync::Arc::make_mut(&mut snapshot.project_tracks);
    let (tracks, master, buses) = (
        &mut project_tracks.tracks,
        &mut project_tracks.master,
        &mut project_tracks.buses,
    );
    for track in tracks
        .iter_mut()
        .chain(std::iter::once(master))
        .chain(buses.iter_mut())
    {
        let track_id = track.id;
        for (chain_pos, effect) in track.effects.iter_mut().enumerate() {
            if let Some(dev) = &effect.plugin_ref {
                let mut dev = dev.clone();
                if let Some(state) = capture_state(PluginGuiKey::Effect {
                    track_id,
                    effect_id: effect.id,
                }) {
                    dev.state_b64 = Some(state);
                }
                effect.plugin_ref = Some(dev.clone());
                effect.external_inputs.clear();
                effect.latency_samples = None;
                effect.reconfiguration_failed = false;
                effect.descriptors = &[];
                effect.has_plugin_gui = false;
                requests.effects.push((track_id, effect.id, chain_pos, dev));
            }
        }
        if let Some(dev) = &track.plugin_instrument_ref {
            track.instrument_latency_samples = None;
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
    use crate::state::{ArrangementTimeline, ProjectTrack, ProjectTracksState, UiEffect};
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
            reconfiguration_failed: false,
            latency_samples: Some(0),
            inactive_sidechains: Default::default(),
            sidechains: Default::default(),
            external_inputs: Default::default(),
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
        let mut track = ProjectTrack::new(TrackId::new(), "T1".to_string(), 0);
        track.effects = effects;
        ProjectSnapshot {
            project_tracks: std::sync::Arc::new(ProjectTracksState {
                reduced_latency_monitoring: false,
                tracks: vec![track],
                master: crate::state::new_master_track(),
                buses: Vec::new(),
                next_track_number: 2,
            }),
            arrange_timeline: std::sync::Arc::new(ArrangementTimeline::default()),
            sections: std::sync::Arc::new(crate::domains::perform::SectionStore::default()),
            launcher_clips: Default::default(),
            bpm: 120.0,
            project_swing: vibez_core::perform::SwingAmount::default(),
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
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
        assert_eq!(snap.project_tracks.tracks[0].effects.len(), 3);
        assert_eq!(
            snap.project_tracks.tracks[0].effects[1]
                .plugin_ref
                .as_ref()
                .unwrap()
                .name,
            "comp"
        );
        assert!(snap.project_tracks.tracks[0].effects[1]
            .external_inputs
            .is_empty());
    }

    #[test]
    fn deleted_or_replaced_placeholder_rejects_its_completed_reload() {
        let device = plugin_device("compressor");
        let placeholder = effect(Some(device.clone()));
        let id = placeholder.id;
        let mut snap = snapshot_with(vec![placeholder]);
        let track = &mut std::sync::Arc::make_mut(&mut snap.project_tracks).tracks[0];
        assert!(accepts_effect_load(Some(track), id, true, &device));
        track.effects[0].plugin_ref = Some(plugin_device("replacement"));
        assert!(!accepts_effect_load(Some(track), id, true, &device));
        track.effects.clear();
        assert!(!accepts_effect_load(Some(track), id, true, &device));
        assert!(accepts_effect_load(Some(track), id, false, &device));
        assert!(!accepts_effect_load(None, id, false, &device));
    }

    #[test]
    fn a_new_plugin_load_needs_room_in_the_device_chain() {
        let device = plugin_device("compressor");
        let id = vibez_core::id::EffectId::new();
        let mut track = crate::state::ProjectTrack::new(TrackId::new(), "Full".into(), 0);
        let template = effect(Some(device.clone()));
        while track.has_device_room() {
            let mut slot = template.clone();
            slot.id = vibez_core::id::EffectId::new();
            track.effects.push(slot);
        }
        assert!(!accepts_effect_load(Some(&track), id, false, &device));
        track.effects.pop();
        assert!(accepts_effect_load(Some(&track), id, false, &device));
    }

    #[test]
    fn live_state_capture_overrides_stale_blob() {
        let mut snap = snapshot_with(vec![effect(Some(plugin_device("eq")))]);
        let requests = collect_plugin_reload_requests(&mut snap, |_| Some("fresh".to_string()));
        assert_eq!(requests.effects[0].3.state_b64.as_deref(), Some("fresh"));
    }

    fn snapshot_with_plugin_instrument() -> (ProjectSnapshot, TrackId) {
        let mut snap = snapshot_with(Vec::new());
        let track = &mut std::sync::Arc::make_mut(&mut snap.project_tracks).tracks[0];
        track.has_instrument = true;
        track.plugin_instrument_name = Some("synth".to_string());
        track.plugin_instrument_ref = Some(plugin_device("synth"));
        let track_id = track.id;
        (snap, track_id)
    }

    fn recorded_instrument_state(snap: &ProjectSnapshot) -> Option<&str> {
        snap.project_tracks.tracks[0]
            .plugin_instrument_ref
            .as_ref()
            .and_then(|device| device.state_b64.as_deref())
    }

    #[test]
    fn a_departing_plugin_instrument_leaves_its_live_state_in_the_undo_step() {
        let (mut snap, track_id) = snapshot_with_plugin_instrument();
        record_plugin_instrument_state(&mut snap, track_id, |key| {
            (key == PluginGuiKey::Instrument { track_id }).then(|| "live".to_string())
        });
        assert_eq!(recorded_instrument_state(&snap), Some("live"));
    }

    #[test]
    fn the_snapshot_undo_and_redo_push_holds_every_live_plugins_state() {
        let (mut snap, _) = snapshot_with_plugin_instrument();
        std::sync::Arc::make_mut(&mut snap.project_tracks).tracks[0]
            .effects
            .push(effect(Some(plugin_device("eq"))));
        record_live_plugin_states(&mut snap, |key| {
            Some(match key {
                PluginGuiKey::Instrument { .. } => "live-synth".to_string(),
                PluginGuiKey::Effect { .. } => "live-eq".to_string(),
            })
        });
        assert_eq!(recorded_instrument_state(&snap), Some("live-synth"));
        assert_eq!(
            snap.project_tracks.tracks[0].effects[0]
                .plugin_ref
                .as_ref()
                .and_then(|device| device.state_b64.as_deref()),
            Some("live-eq")
        );
    }

    #[test]
    fn a_departing_plugin_without_a_live_instance_keeps_its_recorded_state() {
        let (mut snap, track_id) = snapshot_with_plugin_instrument();
        record_plugin_instrument_state(&mut snap, track_id, |_| None);
        assert_eq!(recorded_instrument_state(&snap), Some("stale"));
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
    fn recent_projects_submenu_follows_the_file_menu_and_can_be_cleared() {
        let mut project = ProjectState {
            recent_project_paths: vec!["/projects/a.vzp".into()],
            ..Default::default()
        };

        project.update(ProjectMsg::ToggleFileMenu, ctx());
        project.update(ProjectMsg::ToggleRecentProjects, ctx());
        assert!(project.file_menu_open);
        assert!(project.recent_projects_open);

        let action = project.update(ProjectMsg::ClearRecentProjects, ctx());
        assert!(project.recent_project_paths.is_empty());
        assert!(!project.recent_projects_open);
        assert!(action.persist_ui_settings);

        project.update(ProjectMsg::ToggleFileMenu, ctx());
        assert!(!project.file_menu_open);
        assert!(!project.recent_projects_open);
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
    fn undo_waits_for_an_open_project_transaction() {
        let mut p = ProjectState::default();
        assert!(p
            .history
            .begin_transaction(snapshot_with(Vec::new()), false));

        let action = p.update(ProjectMsg::Undo, ctx());

        assert!(action.apply_snapshot.is_none());
        assert_eq!(
            action.status.as_deref(),
            Some("Finish the current project transaction first")
        );
        assert!(p.history.transaction_active());
    }

    #[test]
    fn plugin_instrument_is_requested_and_fields_kept() {
        let mut snap = snapshot_with(Vec::new());
        let track = &mut std::sync::Arc::make_mut(&mut snap.project_tracks).tracks[0];
        track.plugin_instrument_ref = Some(plugin_device("synth"));
        track.has_instrument = true;
        let requests = collect_plugin_reload_requests(&mut snap, |_| None);
        assert_eq!(requests.instruments.len(), 1);
        assert!(snap.project_tracks.tracks[0]
            .plugin_instrument_ref
            .is_some());
        assert!(snap.project_tracks.tracks[0].has_instrument);
    }
}
