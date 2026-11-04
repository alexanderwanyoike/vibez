//! Clip edit shortcuts over several lanes make one edit and one undo step.

use super::*;
use crate::domains::project::ProjectMsg;
use crate::state::{ArrangementSelection, ProjectTrack, UiNoteClip};
use iced::keyboard::{Key, Modifiers};
use vibez_core::id::{ClipId, TrackId};
use vibez_core::perform::GrooveGrid;

fn note_clip(position_beats: f64) -> UiNoteClip {
    UiNoteClip {
        id: ClipId::new(),
        name: "Pattern".into(),
        position_beats,
        duration_beats: 8.0,
        notes: Vec::new(),
        selected_notes: Default::default(),
        start_marker_beats: 0.0,
        loop_enabled: false,
        loop_start_beats: 0.0,
        loop_end_beats: 8.0,
        groove_grid: GrooveGrid::Off,
    }
}

/// One note clip at beat 0 on each of `lanes` MIDI tracks, all selected.
fn selected_clips_on(lanes: usize) -> (App, Vec<TrackId>) {
    let mut app = super::test_support::app();
    let mut tracks = Vec::new();
    for index in 0..lanes {
        let mut track = ProjectTrack::new(TrackId::new(), format!("MIDI {index}"), 0);
        track.kind = vibez_core::midi::TrackKind::Midi;
        let track_id = track.id;
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(track);
        let clip = note_clip(0.0);
        let clip_id = clip.id;
        app.state
            .arrange_content_mut(track_id)
            .note_clips
            .push(clip);
        app.state
            .arrangement
            .selected_clips
            .insert(ArrangementSelection::NoteClip { track_id, clip_id });
        tracks.push(track_id);
    }
    (app, tracks)
}

fn command_key(app: &mut App, letter: &str) {
    let command = if ON_MACOS {
        Modifiers::LOGO
    } else {
        Modifiers::CTRL
    };
    let message = global_key_handler(Key::Character(letter.into()), command)
        .unwrap_or_else(|| panic!("Command+{letter} is a global shortcut"));
    let _ = app.update(message);
}

fn clip_counts(app: &App, tracks: &[TrackId]) -> Vec<usize> {
    tracks
        .iter()
        .map(|id| {
            app.state
                .arrange_content(*id)
                .map_or(0, |c| c.note_clips.len())
        })
        .collect()
}

#[test]
fn duplicate_over_three_lanes_copies_each_clip_once_in_one_undo_step() {
    let (mut app, tracks) = selected_clips_on(3);

    command_key(&mut app, "d");

    assert_eq!(clip_counts(&app, &tracks), vec![2, 2, 2]);
    assert_eq!(app.state.project.history.undo.len(), 1);
    let _ = app.update(Message::Project(ProjectMsg::Undo));
    assert_eq!(clip_counts(&app, &tracks), vec![1, 1, 1]);
}
