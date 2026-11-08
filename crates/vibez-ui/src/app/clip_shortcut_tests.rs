//! Clip edit shortcuts over several lanes make one edit and one undo step.

use super::*;
use crate::domains::project::ProjectMsg;
use crate::state::{ArrangementSelection, ProjectTrack, UiNoteClip};
use iced::keyboard::{Key, Modifiers};
use vibez_core::id::TrackId;

fn note_clip(position_beats: f64) -> UiNoteClip {
    UiNoteClip {
        name: "Pattern".into(),
        ..UiNoteClip::test_clip(position_beats, 8.0)
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

#[test]
fn clip_edit_shortcuts_that_change_nothing_leave_no_undo_step() {
    let (mut app, _) = selected_clips_on(1);
    // The playhead sits on the clip's start, so there is nothing to split,
    // and a lone clip has nothing to join.
    command_key(&mut app, "e");
    command_key(&mut app, "j");
    app.state.arrangement.selected_clips.clear();
    command_key(&mut app, "d");

    assert!(!app.state.project.history.can_undo());
    assert!(!app.state.project.dirty);
}

#[test]
fn split_over_three_lanes_cuts_each_clip_once_in_one_undo_step() {
    let (mut app, tracks) = selected_clips_on(3);
    let transport = &mut app.state.transport;
    let samples_per_beat = 60.0 * transport.sample_rate as f64 / transport.bpm;
    transport.position_samples = (4.0 * samples_per_beat).round() as u64;

    command_key(&mut app, "e");

    assert_eq!(clip_counts(&app, &tracks), vec![2, 2, 2]);
    assert_eq!(app.state.project.history.undo.len(), 1);
    let _ = app.update(Message::Project(ProjectMsg::Undo));
    assert_eq!(clip_counts(&app, &tracks), vec![1, 1, 1]);
}

#[test]
fn join_makes_one_clip_in_one_undo_step() {
    let (mut app, tracks) = selected_clips_on(1);
    let track_id = tracks[0];
    let clip = note_clip(8.0);
    let clip_id = clip.id;
    app.state
        .arrange_content_mut(track_id)
        .note_clips
        .push(clip);
    app.state
        .arrangement
        .selected_clips
        .insert(ArrangementSelection::NoteClip { track_id, clip_id });

    command_key(&mut app, "j");

    assert_eq!(clip_counts(&app, &tracks), vec![1]);
    assert_eq!(app.state.project.history.undo.len(), 1);
    let _ = app.update(Message::Project(ProjectMsg::Undo));
    assert_eq!(clip_counts(&app, &tracks), vec![2]);
}

#[tokio::test]
async fn audio_join_makes_one_clip_in_one_undo_step() {
    let mut app = super::test_support::app();
    let track = ProjectTrack::new(TrackId::new(), "Audio".into(), 0);
    let track_id = track.id;
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(track);
    for position in [0, 100] {
        let clip = crate::state::UiClip::test_clip(position, 100);
        app.state
            .arrangement
            .selected_clips
            .insert(ArrangementSelection::AudioClip {
                track_id,
                clip_id: clip.id,
            });
        app.state.arrange_content_mut(track_id).clips.push(clip);
    }

    super::test_support::join_selected_audio_clips(&mut app).await;

    let clips = &app.state.arrange_content(track_id).unwrap().clips;
    assert_eq!(clips.len(), 1);
    assert_ne!(clips[0].source, crate::state::UiClip::test_source());
    assert_eq!(app.state.project.history.undo.len(), 1);
    let _ = app.update(Message::Project(ProjectMsg::Undo));
    assert_eq!(app.state.arrange_content(track_id).unwrap().clips.len(), 2);
}
