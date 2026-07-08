//! Automation domain: lane and point editing.
//!
//! Lanes live on [`UiTrack`] (the shared model) and mirror to the
//! engine as whole-lane upserts: every edit clones the lane's points
//! into a [`EngineCommand::SetAutomationLane`], so the allocation
//! happens here on the UI thread and the audio thread only swaps
//! vectors.

use std::collections::HashSet;

use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
use vibez_core::id::{LaneId, TrackId};
use vibez_engine::commands::EngineCommand;

use super::EngineHandle;
use crate::state::UiTrack;

/// Messages the automation domain handles.
#[derive(Debug, Clone)]
pub enum AutomationMsg {
    /// Show/hide the lane strip under a track (view-only).
    ToggleTrackLanes(TrackId),
    AddLane {
        track_id: TrackId,
        target: AutomationTarget,
    },
    RemoveLane {
        track_id: TrackId,
        lane_id: LaneId,
    },
    AddPoint {
        track_id: TrackId,
        lane_id: LaneId,
        beat: f64,
        value: f32,
    },
    /// Move an existing point (drag release). The point keeps beat
    /// order, so its index may change; selection follows it.
    MovePoint {
        track_id: TrackId,
        lane_id: LaneId,
        index: usize,
        beat: f64,
        value: f32,
    },
    RemovePoint {
        track_id: TrackId,
        lane_id: LaneId,
        index: usize,
    },
    SelectPoint {
        track_id: TrackId,
        lane_id: LaneId,
        index: Option<usize>,
    },
    /// Delete-key routing (higher priority than clip deletion when a
    /// point is selected).
    DeleteSelectedPoint,
    /// Set the curve of the segment leaving point `index` (drag
    /// release of an alt-drag; -1..1, 0 = linear).
    SetCurve {
        track_id: TrackId,
        lane_id: LaneId,
        index: usize,
        curve: f32,
    },
    /// Remove every point in a beat range (ctrl-drag erase).
    RemovePointsInRange {
        track_id: TrackId,
        lane_id: LaneId,
        start_beat: f64,
        end_beat: f64,
    },
    OpenLanePicker(TrackId),
    LanePickerQuery(String),
    CloseLanePicker,
}

impl AutomationMsg {
    /// Whether this message edits the project (drives the dirty flag).
    pub fn marks_dirty(&self) -> bool {
        !matches!(
            self,
            AutomationMsg::ToggleTrackLanes(_)
                | AutomationMsg::SelectPoint { .. }
                | AutomationMsg::OpenLanePicker(_)
                | AutomationMsg::LanePickerQuery(_)
                | AutomationMsg::CloseLanePicker
        )
    }
}

/// Cross-domain effects requested by an automation update.
#[derive(Debug, Default, PartialEq)]
pub struct AutomationAction {
    /// Status bar text.
    pub status: Option<String>,
}

/// Automation domain slice.
#[derive(Debug, Default)]
pub struct AutomationState {
    /// Tracks whose lane strip is expanded.
    pub expanded: HashSet<TrackId>,
    /// The selected point: (track, lane, point index).
    pub selected: Option<(TrackId, LaneId, usize)>,
    /// The open add-lane picker, with its search query.
    pub picker: Option<(TrackId, String)>,
}

fn find_lane_mut(
    tracks: &mut [UiTrack],
    track_id: TrackId,
    lane_id: LaneId,
) -> Option<&mut AutomationLane> {
    tracks
        .iter_mut()
        .find(|t| t.id == track_id)?
        .automation
        .iter_mut()
        .find(|l| l.id == lane_id)
}

fn sync_lane(engine: &mut impl EngineHandle, track_id: TrackId, lane: &AutomationLane) {
    engine.send(EngineCommand::SetAutomationLane {
        track_id,
        lane: lane.clone(),
    });
}

/// The target's CURRENT (un-automated) value, normalized 0..1.
/// This seeds new lanes and draws the reference line. Track gain's
/// native range is 0..2 (see the arrangement domain clamp).
pub fn normalized_target_value(target: &AutomationTarget, track: &UiTrack) -> Option<f32> {
    match target {
        AutomationTarget::TrackGain => Some((track.gain / 2.0).clamp(0.0, 1.0)),
        AutomationTarget::TrackPan => Some(track.pan.clamp(0.0, 1.0)),
        AutomationTarget::EffectParam {
            effect_id,
            param_index,
        } => {
            let effect = track.effects.iter().find(|e| e.id == *effect_id)?;
            let d = effect.descriptors.get(*param_index)?;
            let v = effect
                .params
                .get(*param_index)
                .copied()
                .unwrap_or(d.default);
            Some(((v - d.min) / (d.max - d.min).max(f32::EPSILON)).clamp(0.0, 1.0))
        }
        AutomationTarget::InstrumentParam { param_index } => {
            if !track.plugin_instrument_descriptors.is_empty() {
                let d = track.plugin_instrument_descriptors.get(*param_index)?;
                // Plugin values are not mirrored UI-side; use the default.
                Some(((d.default - d.min) / (d.max - d.min).max(f32::EPSILON)).clamp(0.0, 1.0))
            } else {
                let kind = track.instrument_kind?;
                let d = vibez_instruments::descriptors_for(kind).get(*param_index)?;
                let v = track
                    .instrument_params
                    .get(*param_index)
                    .copied()
                    .unwrap_or(d.default);
                Some(((v - d.min) / (d.max - d.min).max(f32::EPSILON)).clamp(0.0, 1.0))
            }
        }
        AutomationTarget::PluginParam { .. } => None,
    }
}

/// The static descriptor behind a target, when one exists (effect
/// and instrument params). Gain/pan have implicit ranges.
pub fn target_descriptor(
    target: &AutomationTarget,
    track: &UiTrack,
) -> Option<&'static vibez_core::effect::ParamDescriptor> {
    match target {
        AutomationTarget::EffectParam {
            effect_id,
            param_index,
        } => track
            .effects
            .iter()
            .find(|e| e.id == *effect_id)?
            .descriptors
            .get(*param_index),
        AutomationTarget::InstrumentParam { param_index } => {
            if !track.plugin_instrument_descriptors.is_empty() {
                track.plugin_instrument_descriptors.get(*param_index)
            } else {
                vibez_instruments::descriptors_for(track.instrument_kind?).get(*param_index)
            }
        }
        _ => None,
    }
}

/// Human-readable name for a lane target, for lane headers and the
/// add-lane picker.
pub fn target_label(target: &AutomationTarget, track: &UiTrack) -> String {
    match target {
        AutomationTarget::TrackGain => "Volume".to_string(),
        AutomationTarget::TrackPan => "Pan".to_string(),
        AutomationTarget::EffectParam {
            effect_id,
            param_index,
        } => {
            let Some(effect) = track.effects.iter().find(|e| e.id == *effect_id) else {
                return "Missing effect".to_string();
            };
            let effect_name = effect
                .plugin_name
                .clone()
                .unwrap_or_else(|| format!("{:?}", effect.effect_type));
            match effect.descriptors.get(*param_index) {
                Some(d) => format!("{effect_name}: {}", d.name),
                None => format!("{effect_name}: P{param_index}"),
            }
        }
        AutomationTarget::InstrumentParam { param_index } => {
            let (name, descriptors) = if !track.plugin_instrument_descriptors.is_empty() {
                (
                    track.plugin_instrument_name.as_deref().unwrap_or("Plugin"),
                    track.plugin_instrument_descriptors,
                )
            } else {
                (
                    "Instrument",
                    track
                        .instrument_kind
                        .map(vibez_instruments::descriptors_for)
                        .unwrap_or(&[]),
                )
            };
            match descriptors.get(*param_index) {
                Some(d) => format!("{name}: {}", d.name),
                None => format!("{name} P{param_index}"),
            }
        }
        AutomationTarget::PluginParam { .. } => "Plugin param".to_string(),
    }
}

impl AutomationState {
    pub fn update(
        &mut self,
        msg: AutomationMsg,
        engine: &mut impl EngineHandle,
        tracks: &mut [UiTrack],
    ) -> AutomationAction {
        let mut action = AutomationAction::default();
        match msg {
            AutomationMsg::ToggleTrackLanes(track_id) => {
                if !self.expanded.remove(&track_id) {
                    self.expanded.insert(track_id);
                }
            }
            AutomationMsg::AddLane { track_id, target } => {
                let Some(track) = tracks.iter_mut().find(|t| t.id == track_id) else {
                    return action;
                };
                if track.automation.iter().any(|l| l.target == target) {
                    action.status = Some("That parameter already has a lane".to_string());
                    return action;
                }
                let mut lane = AutomationLane::new(target);
                // Start where the knob already is.
                if let Some(value) = normalized_target_value(&target, track) {
                    lane.insert_point(AutomationPoint {
                        beat: 0.0,
                        value,
                        curve: 0.0,
                    });
                }
                sync_lane(engine, track_id, &lane);
                let label = target_label(&target, track);
                track.automation.push(lane);
                self.expanded.insert(track_id);
                self.picker = None;
                action.status = Some(format!("Added automation lane: {label}"));
            }
            AutomationMsg::RemoveLane { track_id, lane_id } => {
                if let Some(track) = tracks.iter_mut().find(|t| t.id == track_id) {
                    track.automation.retain(|l| l.id != lane_id);
                }
                engine.send(EngineCommand::RemoveAutomationLane { track_id, lane_id });
                if matches!(self.selected, Some((t, l, _)) if t == track_id && l == lane_id) {
                    self.selected = None;
                }
                action.status = Some("Removed automation lane".to_string());
            }
            AutomationMsg::AddPoint {
                track_id,
                lane_id,
                beat,
                value,
            } => {
                let Some(lane) = find_lane_mut(tracks, track_id, lane_id) else {
                    return action;
                };
                let point = AutomationPoint {
                    beat: beat.max(0.0),
                    value: value.clamp(0.0, 1.0),
                    curve: 0.0,
                };
                lane.insert_point(point);
                let index = lane
                    .points
                    .iter()
                    .position(|p| p.beat == point.beat)
                    .unwrap_or(0);
                let lane = lane.clone();
                sync_lane(engine, track_id, &lane);
                self.selected = Some((track_id, lane_id, index));
            }
            AutomationMsg::MovePoint {
                track_id,
                lane_id,
                index,
                beat,
                value,
            } => {
                let Some(lane) = find_lane_mut(tracks, track_id, lane_id) else {
                    return action;
                };
                if index >= lane.points.len() {
                    return action;
                }
                let curve = lane.points[index].curve;
                lane.points.remove(index);
                let point = AutomationPoint {
                    beat: beat.max(0.0),
                    value: value.clamp(0.0, 1.0),
                    curve,
                };
                lane.insert_point(point);
                let new_index = lane
                    .points
                    .iter()
                    .position(|p| p.beat == point.beat)
                    .unwrap_or(0);
                let lane = lane.clone();
                sync_lane(engine, track_id, &lane);
                self.selected = Some((track_id, lane_id, new_index));
            }
            AutomationMsg::RemovePoint {
                track_id,
                lane_id,
                index,
            } => {
                let Some(lane) = find_lane_mut(tracks, track_id, lane_id) else {
                    return action;
                };
                if index >= lane.points.len() {
                    return action;
                }
                lane.points.remove(index);
                let lane = lane.clone();
                sync_lane(engine, track_id, &lane);
                if matches!(self.selected, Some((t, l, i)) if t == track_id && l == lane_id && i == index)
                {
                    self.selected = None;
                }
            }
            AutomationMsg::SelectPoint {
                track_id,
                lane_id,
                index,
            } => {
                self.selected = index.map(|i| (track_id, lane_id, i));
            }
            AutomationMsg::SetCurve {
                track_id,
                lane_id,
                index,
                curve,
            } => {
                let Some(lane) = find_lane_mut(tracks, track_id, lane_id) else {
                    return action;
                };
                if index >= lane.points.len() {
                    return action;
                }
                lane.points[index].curve = curve.clamp(-1.0, 1.0);
                let lane = lane.clone();
                sync_lane(engine, track_id, &lane);
            }
            AutomationMsg::RemovePointsInRange {
                track_id,
                lane_id,
                start_beat,
                end_beat,
            } => {
                let (lo, hi) = if start_beat <= end_beat {
                    (start_beat, end_beat)
                } else {
                    (end_beat, start_beat)
                };
                let Some(lane) = find_lane_mut(tracks, track_id, lane_id) else {
                    return action;
                };
                let before = lane.points.len();
                lane.points.retain(|p| p.beat < lo || p.beat > hi);
                if lane.points.len() != before {
                    let lane = lane.clone();
                    sync_lane(engine, track_id, &lane);
                    if matches!(self.selected, Some((t, l, _)) if t == track_id && l == lane_id) {
                        self.selected = None;
                    }
                    action.status = Some(format!("Erased {} point(s)", before - lane.points.len()));
                }
            }
            AutomationMsg::OpenLanePicker(track_id) => {
                self.picker = Some((track_id, String::new()));
            }
            AutomationMsg::LanePickerQuery(query) => {
                if let Some((_, q)) = &mut self.picker {
                    *q = query;
                }
            }
            AutomationMsg::CloseLanePicker => {
                self.picker = None;
            }
            AutomationMsg::DeleteSelectedPoint => {
                if let Some((track_id, lane_id, index)) = self.selected.take() {
                    return self.update(
                        AutomationMsg::RemovePoint {
                            track_id,
                            lane_id,
                            index,
                        },
                        engine,
                        tracks,
                    );
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

    fn track() -> Vec<UiTrack> {
        vec![UiTrack::new(TrackId::new(), "T1".to_string(), 0)]
    }

    #[test]
    fn add_lane_syncs_and_expands() {
        let mut a = AutomationState::default();
        let mut engine = RecordingEngine::default();
        let mut tracks = track();
        let tid = tracks[0].id;
        a.update(
            AutomationMsg::AddLane {
                track_id: tid,
                target: AutomationTarget::TrackGain,
            },
            &mut engine,
            &mut tracks,
        );
        assert_eq!(tracks[0].automation.len(), 1);
        assert!(a.expanded.contains(&tid));
        assert!(matches!(
            engine.0[0],
            EngineCommand::SetAutomationLane { .. }
        ));

        // Duplicate target refused.
        let action = a.update(
            AutomationMsg::AddLane {
                track_id: tid,
                target: AutomationTarget::TrackGain,
            },
            &mut engine,
            &mut tracks,
        );
        assert_eq!(tracks[0].automation.len(), 1);
        assert!(action.status.unwrap().contains("already"));
    }

    #[test]
    fn point_lifecycle_add_move_delete() {
        let mut a = AutomationState::default();
        let mut engine = RecordingEngine::default();
        let mut tracks = track();
        let tid = tracks[0].id;
        a.update(
            AutomationMsg::AddLane {
                track_id: tid,
                target: AutomationTarget::TrackGain,
            },
            &mut engine,
            &mut tracks,
        );
        let lane_id = tracks[0].automation[0].id;

        a.update(
            AutomationMsg::AddPoint {
                track_id: tid,
                lane_id,
                beat: 8.0,
                value: 0.25,
            },
            &mut engine,
            &mut tracks,
        );
        a.update(
            AutomationMsg::AddPoint {
                track_id: tid,
                lane_id,
                beat: 0.0,
                value: 1.0,
            },
            &mut engine,
            &mut tracks,
        );
        let beats: Vec<f64> = tracks[0].automation[0]
            .points
            .iter()
            .map(|p| p.beat)
            .collect();
        assert_eq!(beats, vec![0.0, 8.0]);
        assert_eq!(a.selected, Some((tid, lane_id, 0)));

        // Drag point 0 past point 1: selection follows to the end.
        a.update(
            AutomationMsg::MovePoint {
                track_id: tid,
                lane_id,
                index: 0,
                beat: 16.0,
                value: 0.5,
            },
            &mut engine,
            &mut tracks,
        );
        let beats: Vec<f64> = tracks[0].automation[0]
            .points
            .iter()
            .map(|p| p.beat)
            .collect();
        assert_eq!(beats, vec![8.0, 16.0]);
        assert_eq!(a.selected, Some((tid, lane_id, 1)));

        // Delete key removes the selected point.
        a.update(AutomationMsg::DeleteSelectedPoint, &mut engine, &mut tracks);
        assert_eq!(tracks[0].automation[0].points.len(), 1);
        assert_eq!(a.selected, None);
    }

    #[test]
    fn remove_lane_clears_engine_and_selection() {
        let mut a = AutomationState::default();
        let mut engine = RecordingEngine::default();
        let mut tracks = track();
        let tid = tracks[0].id;
        a.update(
            AutomationMsg::AddLane {
                track_id: tid,
                target: AutomationTarget::TrackPan,
            },
            &mut engine,
            &mut tracks,
        );
        let lane_id = tracks[0].automation[0].id;
        a.selected = Some((tid, lane_id, 0));
        a.update(
            AutomationMsg::RemoveLane {
                track_id: tid,
                lane_id,
            },
            &mut engine,
            &mut tracks,
        );
        assert!(tracks[0].automation.is_empty());
        assert_eq!(a.selected, None);
        assert!(matches!(
            engine.0.last(),
            Some(EngineCommand::RemoveAutomationLane { .. })
        ));
    }

    #[test]
    fn values_clamp_to_unit_range() {
        let mut a = AutomationState::default();
        let mut engine = RecordingEngine::default();
        let mut tracks = track();
        let tid = tracks[0].id;
        a.update(
            AutomationMsg::AddLane {
                track_id: tid,
                target: AutomationTarget::TrackGain,
            },
            &mut engine,
            &mut tracks,
        );
        let lane_id = tracks[0].automation[0].id;
        a.update(
            AutomationMsg::AddPoint {
                track_id: tid,
                lane_id,
                beat: -3.0,
                value: 7.0,
            },
            &mut engine,
            &mut tracks,
        );
        let p = tracks[0].automation[0].points[0];
        assert_eq!(p.beat, 0.0);
        assert_eq!(p.value, 1.0);
    }
}
