//! The grid shortcuts, Z and X act on the editor that has focus.

use super::*;
use crate::state::{
    ArrangementSelection, DetailPanelTab, GridConfig, ProjectTrack, UiClip, UiNoteClip,
};
use iced::keyboard::{Key, Modifiers};
use vibez_core::id::TrackId;

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
        name: "Pattern".into(),
        ..UiNoteClip::test_clip(0.0, 8.0)
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

/// An app with one audio track whose one clip, `clip`, is selected, which
/// shows it in the audio editor.
fn app_with_open_audio_clip(clip: UiClip) -> App {
    let mut app = super::test_support::app();
    app.state.view.workspace = crate::state::Workspace::Arrange;
    app.state.view.detail_panel_tab = DetailPanelTab::Clip;
    let track = ProjectTrack::new(TrackId::new(), "Audio".into(), 0);
    let track_id = track.id;
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(track);
    let clip_id = clip.id;
    app.state.arrange_content_mut(track_id).clips.push(clip);
    app.state.arrangement.selected_track = Some(track_id);
    app.state
        .arrangement
        .selected_clips
        .insert(ArrangementSelection::AudioClip { track_id, clip_id });
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
fn command_one_with_the_audio_editor_open_narrows_the_clip_grid() {
    let mut app = app_with_open_audio_clip(UiClip::test_clip(0, 48_000));

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

fn press(app: &mut App, key: &str) {
    let message = global_key_handler(Key::Character(key.into()), Modifiers::empty())
        .expect("Z and X are global shortcuts");
    let _ = app.update(message);
}

#[test]
fn z_frames_the_open_audio_editor_on_its_loop_and_x_goes_back() {
    let beat = super::test_support::app()
        .state
        .transport
        .samples_per_beat() as u64;
    let mut clip = UiClip::test_clip(0, 64 * beat);
    clip.loop_enabled = true;
    clip.loop_start = 8 * beat;
    clip.loop_end = 16 * beat;
    clip.start_marker = clip.loop_start;
    let mut app = app_with_open_audio_clip(clip);
    let arrange = app.state.view.viewport;
    let whole_sample = app.clip_editor_pixels_per_beat();

    press(&mut app, "z");

    assert_eq!(app.state.view.viewport, arrange, "Arrange stays put");
    let loop_framed = app.clip_editor_pixels_per_beat();
    assert!(
        loop_framed > 4.0 * whole_sample,
        "{loop_framed} frames the 8 beat loop, not the 64 beat sample at {whole_sample}"
    );

    press(&mut app, "x");

    assert_eq!(app.clip_editor_pixels_per_beat(), whole_sample);
}

#[test]
fn z_frames_the_opened_note_clip_on_its_loop_and_x_goes_back() {
    let mut app = app_with_note_clip(true);
    let (track_id, clip_id) = app.state.arrangement.selected_note_clip.unwrap();
    let clip = &mut app.state.arrange_content_mut(track_id).note_clips[0];
    clip.loop_enabled = true;
    clip.loop_start_beats = 2.0;
    clip.loop_end_beats = 4.0;
    let _ = app.update(Message::set_clip_editor_view(
        clip_id,
        crate::timeline_geometry::TimelineViewport::new(0.5, 0.0),
    ));
    let arrange = app.state.view.viewport;
    let zoomed_out = app.clip_editor_pixels_per_beat();

    press(&mut app, "z");

    assert_eq!(app.state.view.viewport, arrange, "Arrange stays put");
    assert!(app.clip_editor_pixels_per_beat() > 4.0 * zoomed_out);

    press(&mut app, "x");

    assert_eq!(app.clip_editor_pixels_per_beat(), zoomed_out);
}

#[test]
fn z_without_an_open_clip_editor_frames_the_arrange_selection() {
    let mut app = app_with_note_clip(false);
    app.state.arrangement.time_selection_active = true;
    app.state.arrangement.selection_start_beats = 4.0;
    app.state.arrangement.selection_end_beats = 8.0;
    let arrange = app.state.view.viewport;

    press(&mut app, "z");
    assert_ne!(app.state.view.viewport, arrange);

    press(&mut app, "x");
    assert_eq!(app.state.view.viewport, arrange);
}
