//! The drop zone a Browser drag shows over the device it would land on.

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
    fn the_arrangement_marks_its_own_drops_without_a_zone() {
        let track_id = TrackId::new();
        for target in [
            BrowserDropTarget::ArrangementLane {
                track_id,
                beat: 1.0,
                compatible: true,
            },
            BrowserDropTarget::ArrangementLane {
                track_id,
                beat: 1.0,
                compatible: false,
            },
            BrowserDropTarget::EmptyArrangement { beat: 1.0 },
        ] {
            assert_eq!(target.drop_zone(), None);
        }
    }
}
