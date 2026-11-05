//! The grid shortcuts edit the grid of the editor that has focus.

use super::*;
use crate::state::{GridConfig, ProjectTrack, UiNoteClip};
use iced::keyboard::{Key, Modifiers};
use vibez_core::id::{ClipId, TrackId};
use vibez_core::perform::GrooveGrid;

/// An app with one MIDI track holding one note clip, and whether that
/// clip is open in the piano roll.
fn app_with_note_clip(open: bool) -> App {
    let mut app = super::test_support::app();
    app.state.view.workspace = crate::state::Workspace::Arrange;
    let mut track = ProjectTrack::new(TrackId::new(), "MIDI".into(), 0);
    track.kind = vibez_core::midi::TrackKind::Midi;
    let track_id = track.id;
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(track);
    let clip = UiNoteClip {
        id: ClipId::new(),
        name: "Pattern".into(),
        position_beats: 0.0,
        duration_beats: 8.0,
        notes: Vec::new(),
        selected_notes: Default::default(),
        start_marker_beats: 0.0,
        loop_enabled: false,
        loop_start_beats: 0.0,
        loop_end_beats: 8.0,
        groove_grid: GrooveGrid::Off,
    };
    let clip_id = clip.id;
    app.state
        .arrange_content_mut(track_id)
        .note_clips
        .push(clip);
    app.state.arrangement.selected_track = Some(track_id);
    if open {
        app.state.arrangement.selected_note_clip = Some((track_id, clip_id));
    }
    app
}

fn narrow_grid(app: &mut App) {
    let command = if ON_MACOS {
        Modifiers::LOGO
    } else {
        Modifiers::CTRL
    };
    let message = global_key_handler(Key::Character("1".into()), command)
        .expect("Command+1 is a global shortcut");
    let _ = app.update(message);
}

#[test]
fn command_one_with_the_piano_roll_focused_narrows_the_clip_grid() {
    let mut app = app_with_note_clip(true);

    narrow_grid(&mut app);

    assert_eq!(app.state.view.arrange_grid, GridConfig::default());
    assert_eq!(app.state.view.clip_grid.adaptive_bias, 1);
}

#[test]
fn command_one_without_an_open_clip_narrows_the_arrange_grid() {
    let mut app = app_with_note_clip(false);

    narrow_grid(&mut app);

    assert_eq!(app.state.view.arrange_grid.adaptive_bias, 1);
    assert_eq!(app.state.view.clip_grid, GridConfig::default());
}

#[test]
fn command_one_with_the_perform_pads_focused_edits_no_grid() {
    let mut app = app_with_note_clip(true);
    app.state.view.workspace = crate::state::Workspace::Perform;
    app.state.perform.editor_focus = crate::domains::perform::PerformEditorFocus::PadSurface;

    narrow_grid(&mut app);

    assert_eq!(app.state.view.arrange_grid, GridConfig::default());
    assert_eq!(app.state.view.clip_grid, GridConfig::default());
}
