//! The drop zone a Browser drag shows over the device it would land on.

use crate::domains::device_panel_drop::DevicePanelDrop;
use crate::state::BrowserDropTarget;

/// What a release in the zone does to the device, for its icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropAction {
    Add,
    Replace,
    Load,
    /// The target will not take the audio.
    Refuse,
}

impl DropAction {
    /// Whether a release here is turned away.
    pub fn refuses(self) -> bool {
        self == Self::Refuse
    }
}

/// The marked place a release lands on and what it says there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropZone {
    pub action: DropAction,
    /// The outcome in the status bar's words, or `None` where the zone is
    /// a Drum Rack pad, too small to hold them.
    pub label: Option<String>,
}

impl BrowserDropTarget {
    /// The zone to mark under the cursor, or `None` on a lane that takes the
    /// audio, which previews the Clip it would place instead.
    pub fn drop_zone(&self) -> Option<DropZone> {
        let action = match self {
            Self::ArrangementLane {
                compatible: true, ..
            } => return None,
            Self::ArrangementLane { .. } => DropAction::Refuse,
            Self::EmptyArrangement { .. } => DropAction::Add,
            Self::DrumRackPad { .. } | Self::DrumRack { .. } => {
                return Some(DropZone {
                    action: DropAction::Load,
                    label: None,
                })
            }
            Self::Sampler { .. } => DropAction::Load,
            Self::DevicePanel { drop, .. } => match drop {
                DevicePanelDrop::AddSampler => DropAction::Add,
                DevicePanelDrop::ReplaceWithSampler { .. } => DropAction::Replace,
                DevicePanelDrop::LoadIntoExisting => DropAction::Load,
            },
        };
        Some(DropZone {
            action,
            label: Some(self.status()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::device_panel_drop::DevicePanelDrop;
    use crate::state::BrowserDropTarget;
    use vibez_core::id::TrackId;

    fn panel(drop: DevicePanelDrop) -> BrowserDropTarget {
        BrowserDropTarget::DevicePanel {
            track_id: TrackId::new(),
            drop,
        }
    }

    #[test]
    fn a_device_panel_zone_names_its_outcome_with_a_matching_action() {
        for (drop, action) in [
            (DevicePanelDrop::AddSampler, DropAction::Add),
            (
                DevicePanelDrop::ReplaceWithSampler {
                    instrument: "FM Synth".into(),
                },
                DropAction::Replace,
            ),
            (DevicePanelDrop::LoadIntoExisting, DropAction::Load),
        ] {
            let target = panel(drop);
            assert_eq!(
                target.drop_zone(),
                Some(DropZone {
                    action,
                    label: Some(target.status()),
                })
            );
        }
    }

    #[test]
    fn a_sampler_card_zone_loads_and_says_so() {
        let target = BrowserDropTarget::Sampler {
            track_id: TrackId::new(),
        };
        assert_eq!(
            target.drop_zone(),
            Some(DropZone {
                action: DropAction::Load,
                label: Some(target.status()),
            })
        );
    }

    #[test]
    fn a_drum_pad_zone_is_too_small_for_words() {
        let track_id = TrackId::new();
        for target in [
            BrowserDropTarget::DrumRackPad {
                track_id,
                pad_index: 3,
            },
            BrowserDropTarget::DrumRack {
                track_id,
                pad_index: 0,
            },
        ] {
            assert_eq!(
                target.drop_zone(),
                Some(DropZone {
                    action: DropAction::Load,
                    label: None,
                })
            );
        }
    }

    #[test]
    fn a_lane_that_takes_the_audio_marks_the_drop_itself() {
        let target = BrowserDropTarget::ArrangementLane {
            track_id: TrackId::new(),
            beat: 1.0,
            compatible: true,
        };
        assert_eq!(target.drop_zone(), None);
    }

    #[test]
    fn a_refused_lane_says_why_in_a_refusal_zone() {
        let target = BrowserDropTarget::ArrangementLane {
            track_id: TrackId::new(),
            beat: 1.0,
            compatible: false,
        };
        assert_eq!(
            target.drop_zone(),
            Some(DropZone {
                action: DropAction::Refuse,
                label: Some(target.status()),
            })
        );
    }

    #[test]
    fn the_new_track_lane_adds_and_says_so() {
        let target = BrowserDropTarget::EmptyArrangement { beat: 2.0 };
        assert_eq!(
            target.drop_zone(),
            Some(DropZone {
                action: DropAction::Add,
                label: Some(target.status()),
            })
        );
    }

    #[test]
    fn only_a_refusal_zone_refuses() {
        assert!(DropAction::Refuse.refuses());
        for action in [DropAction::Add, DropAction::Replace, DropAction::Load] {
            assert!(!action.refuses());
        }
    }
}
