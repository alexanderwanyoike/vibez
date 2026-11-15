//! A MIDI file dragged from the Browser lands only where a MIDI file can.

use std::path::PathBuf;

use super::*;
use crate::state::BrowserDropTarget;
use vibez_core::midi::{InstrumentKind, TrackKind};

fn dragging(file: &str) -> BrowserState {
    let mut browser = BrowserState::default();
    browser.update(BrowserMsg::BeginPendingDrag {
        source: MediaSourceRef::LocalFile {
            path: PathBuf::from("/tmp").join(file),
        },
        label: file.into(),
        origin_x: 0.0,
        origin_y: 0.0,
    });
    browser.update(BrowserMsg::PendingDragMoved { x: 40.0, y: 0.0 });
    browser
}

#[test]
fn a_drag_lands_on_the_lanes_of_its_own_kind() {
    let midi = dragging("Rebeater P3.mid");
    let audio = dragging("kick.wav");

    assert_eq!(midi.drag_media(), Some(DragMedia::Midi));
    assert_eq!(audio.drag_media(), Some(DragMedia::Audio));
    assert!(midi.drag_lands_on(TrackKind::Midi));
    assert!(midi.drag_lands_on(TrackKind::Instrument(InstrumentKind::DrumRack)));
    assert!(!midi.drag_lands_on(TrackKind::Audio));
    assert!(audio.drag_lands_on(TrackKind::Audio));
    assert!(!audio.drag_lands_on(TrackKind::Midi));
    assert!(!BrowserState::default().drag_lands_on(TrackKind::Audio));
}

#[test]
fn a_midi_drag_names_midi_targets() {
    let mut browser = BrowserState::default();
    let action = browser.update(BrowserMsg::BeginPendingDrag {
        source: MediaSourceRef::LocalFile {
            path: PathBuf::from("/tmp/Rebeater P3.mid"),
        },
        label: "Rebeater P3.mid".into(),
        origin_x: 0.0,
        origin_y: 0.0,
    });
    assert_eq!(action.status, None);
    let moving = browser.update(BrowserMsg::PendingDragMoved { x: 40.0, y: 0.0 });
    assert_eq!(
        moving.status.as_deref(),
        Some("Moving Rebeater P3.mid - choose a MIDI lane, empty Arrange, or a MIDI track's Clip slot")
    );

    let track_id = TrackId::new();
    let lane = browser.update(BrowserMsg::DragHoverTrack {
        track_id,
        beat: 2.0,
        compatible: true,
    });
    assert_eq!(lane.status.as_deref(), Some("MIDI lane at beat 2.00"));
    let refused = browser.update(BrowserMsg::DragHoverTrack {
        track_id,
        beat: 2.0,
        compatible: false,
    });
    assert_eq!(
        refused.status.as_deref(),
        Some("MIDI can't go on an audio lane")
    );
    let empty = browser.update(BrowserMsg::DragHoverEmptyArrangement { beat: 1.0 });
    assert_eq!(empty.status.as_deref(), Some("New MIDI track at beat 1.00"));
    assert_eq!(
        browser.drop_zone().and_then(|zone| zone.label),
        Some("New MIDI track at beat 1.00".into())
    );
}

#[test]
fn a_midi_drag_passes_over_instrument_and_device_targets() {
    let track_id = TrackId::new();
    for hover in [
        BrowserMsg::DragHoverSampler { track_id },
        BrowserMsg::DragHoverDrumRackPad {
            track_id,
            pad_index: 0,
        },
        BrowserMsg::DragHoverDrumRack {
            track_id,
            pad_index: 0,
        },
        BrowserMsg::DragHoverDevicePanel {
            track_id,
            drop: DevicePanelDrop::AddSampler,
        },
    ] {
        let mut browser = dragging("Rebeater P3.mid");

        let action = browser.update(hover.clone());

        assert_eq!(browser.drag_target, None, "{hover:?}");
        assert_eq!(
            action.status.as_deref(),
            Some("MIDI files go on a MIDI track, empty Arrange or a MIDI track's Clip slot"),
            "{hover:?}"
        );
    }
}

#[test]
fn an_audio_drag_still_names_audio_targets() {
    let mut browser = dragging("kick.wav");

    let lane = browser.update(BrowserMsg::DragHoverTrack {
        track_id: TrackId::new(),
        beat: 2.0,
        compatible: true,
    });

    assert_eq!(lane.status.as_deref(), Some("Audio lane at beat 2.00"));
    assert!(matches!(
        browser.drag_target,
        Some(BrowserDropTarget::ArrangementLane { .. })
    ));
}

#[test]
fn a_lane_of_the_other_kind_says_why_it_refuses_the_drag() {
    let midi = dragging("Rebeater P3.mid");
    let audio = dragging("kick.wav");

    assert_eq!(
        midi.lane_refusal(TrackKind::Audio).as_deref(),
        Some("MIDI can't go on an audio lane")
    );
    assert_eq!(midi.lane_refusal(TrackKind::Midi), None);
    assert_eq!(
        audio.lane_refusal(TrackKind::Midi).as_deref(),
        Some("Audio can't go on a MIDI lane")
    );
    assert_eq!(BrowserState::default().lane_refusal(TrackKind::Midi), None);
}

#[test]
fn stepping_onto_a_midi_file_loads_no_waveform() {
    let mut browser = BrowserState::default();
    let source = MediaSourceRef::LocalFile {
        path: PathBuf::from("/tmp/Rebeater P3.mid"),
    };

    let action = browser.update(BrowserMsg::SelectSampleBrowserEntry(source.clone()));

    assert_eq!(browser.selected_source, Some(source));
    assert_eq!(action.load_waveform, None);
}

#[test]
fn a_midi_drag_is_previewed_as_midi_not_as_an_audio_import_mode() {
    let midi = dragging("Rebeater P3.mid");
    let audio = dragging("kick.wav");

    assert_eq!(
        midi.drag_detail(120.0).as_deref(),
        Some("Rebeater P3.mid · MIDI")
    );
    assert_eq!(audio.drag_detail(120.0).as_deref(), Some("kick.wav · WARP"));
}
