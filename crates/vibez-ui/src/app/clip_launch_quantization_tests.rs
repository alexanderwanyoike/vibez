//! Each Project Track launches its Clips on its own boundary.

use std::sync::Arc;

use vibez_core::id::{ClipId, TrackId};
use vibez_core::perform::MusicalBoundary;
use vibez_engine::commands::EngineCommand;

use crate::domains::perform::{ClipMsg, PerformMsg};
use crate::message::Message;
use crate::state::{ProjectTrack, Workspace};

fn clip_app(tracks: &[TrackId]) -> (super::App, rtrb::Consumer<EngineCommand>) {
    let mut app = super::test_support::app();
    let (commands, sent) = rtrb::RingBuffer::new(64);
    app.cmd_tx = crate::domains::EngineCommandQueue::new(commands);
    app.state.view.workspace = Workspace::Perform;
    app.state.perform.layout = vibez_project::PerformLayout::Clips;
    for (index, track) in tracks.iter().enumerate() {
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(ProjectTrack::new_instrument(
                *track,
                format!("Track {index}"),
                vibez_core::midi::TrackKind::Midi,
                0,
            ));
        Arc::make_mut(&mut app.state.perform.clips).clips.push(
            crate::domains::perform::clip_record::empty_midi_clip(
                ClipId::new(),
                *track,
                0,
                "Break".into(),
                4.0,
            ),
        );
    }
    (app, sent)
}

fn set(track_id: TrackId, quantization: MusicalBoundary) -> Message {
    Message::Perform(PerformMsg::Clips(ClipMsg::SetLaunchQuantization {
        track_id,
        quantization,
    }))
}

fn launches(sent: &mut rtrb::Consumer<EngineCommand>) -> Vec<(MusicalBoundary, Vec<TrackId>)> {
    std::iter::from_fn(|| sent.pop().ok())
        .filter_map(|command| match command {
            EngineCommand::QueueClips {
                clips,
                quantization,
            } => Some((
                quantization,
                clips.iter().map(|clip| clip.track_id).collect(),
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn a_track_launches_its_clips_on_its_own_quantization() {
    let breaks = TrackId::new();
    let pads = TrackId::new();
    let (mut app, mut sent) = clip_app(&[breaks, pads]);
    let _ = app.update(set(breaks, MusicalBoundary::Eighth));
    assert!(app.state.project.dirty);
    for (track, expected) in [
        (breaks, MusicalBoundary::Eighth),
        (pads, MusicalBoundary::OneBar),
    ] {
        let clip = app.state.perform.clips.at(track, 0).unwrap().id;
        let _ = app.update(Message::Perform(PerformMsg::Clips(ClipMsg::Toggle(clip))));
        assert_eq!(launches(&mut sent), vec![(expected, vec![track])]);
    }
}

#[test]
fn a_row_launch_sends_one_batch_per_quantization() {
    let breaks = TrackId::new();
    let fills = TrackId::new();
    let pads = TrackId::new();
    let (mut app, mut sent) = clip_app(&[breaks, fills, pads]);
    let _ = app.update(set(breaks, MusicalBoundary::Eighth));
    let _ = app.update(set(fills, MusicalBoundary::Eighth));
    let _ = app.update(Message::Perform(PerformMsg::Clips(ClipMsg::LaunchRow(0))));
    assert_eq!(
        launches(&mut sent),
        vec![
            (MusicalBoundary::Eighth, vec![breaks, fills]),
            (MusicalBoundary::OneBar, vec![pads]),
        ]
    );
}

#[tokio::test]
async fn track_launch_quantization_reopens_with_the_project() {
    let breaks = TrackId::new();
    let pads = TrackId::new();
    let (mut app, _sent) = clip_app(&[breaks, pads]);
    let _ = app.update(set(breaks, MusicalBoundary::Sixteenth));
    let project = app.project_from_state();
    assert_eq!(
        project.launcher_tracks,
        vec![vibez_project::LauncherTrackInfo {
            track_id: breaks,
            launch_quantization: MusicalBoundary::Sixteenth,
        }],
        "only tracks off the default are written"
    );
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("jungle.vzp");
    super::save_project_async(path.clone(), None, project)
        .await
        .unwrap();
    let loaded = super::load_project_async(path, None).await.unwrap();
    let mut reopened = super::test_support::app();
    reopened.rebuild_from_loaded_project(loaded);
    let clips = &reopened.state.perform.clips;
    assert_eq!(
        clips.launch_quantization(breaks),
        MusicalBoundary::Sixteenth
    );
    assert_eq!(clips.launch_quantization(pads), MusicalBoundary::OneBar);
}

#[test]
fn undo_restores_the_previous_track_quantization() {
    let breaks = TrackId::new();
    let (mut app, _sent) = clip_app(&[breaks]);
    let _ = app.update(set(breaks, MusicalBoundary::Eighth));
    let _ = app.update(Message::Project(crate::domains::project::ProjectMsg::Undo));
    assert_eq!(
        app.state.perform.clips.launch_quantization(breaks),
        MusicalBoundary::OneBar
    );
}

#[test]
fn choosing_a_tracks_current_quantization_is_not_an_edit() {
    let breaks = TrackId::new();
    let (mut app, _sent) = clip_app(&[breaks]);
    let _ = app.update(set(breaks, MusicalBoundary::OneBar));
    assert!(!app.state.project.dirty);
    assert!(!app.state.project.history.can_undo());
}

fn set_clip(clip_id: ClipId, quantization: Option<MusicalBoundary>) -> Message {
    Message::Perform(PerformMsg::Clips(ClipMsg::SetClipLaunchQuantization {
        clip_id,
        quantization,
    }))
}

#[test]
fn a_clip_launch_quantization_overrides_its_track_until_cleared() {
    let breaks = TrackId::new();
    let (mut app, mut sent) = clip_app(&[breaks]);
    let clip = app.state.perform.clips.at(breaks, 0).unwrap().id;
    let _ = app.update(set(breaks, MusicalBoundary::Eighth));
    let _ = app.update(set_clip(clip, Some(MusicalBoundary::OneBar)));
    assert!(app.state.project.dirty);
    let toggle = Message::Perform(PerformMsg::Clips(ClipMsg::Toggle(clip)));
    let _ = app.update(toggle.clone());
    assert_eq!(
        launches(&mut sent),
        vec![(MusicalBoundary::OneBar, vec![breaks])]
    );
    let _ = app.update(set_clip(clip, None));
    app.state.perform.clip_editor.queued.clear();
    let _ = app.update(toggle);
    assert_eq!(
        launches(&mut sent),
        vec![(MusicalBoundary::Eighth, vec![breaks])],
        "a cleared clip follows its track again"
    );
}

#[test]
fn a_row_launch_groups_clips_by_their_own_quantization() {
    let breaks = TrackId::new();
    let pads = TrackId::new();
    let (mut app, mut sent) = clip_app(&[breaks, pads]);
    let pad = app.state.perform.clips.at(pads, 0).unwrap().id;
    let _ = app.update(set(breaks, MusicalBoundary::Eighth));
    let _ = app.update(set_clip(pad, Some(MusicalBoundary::Eighth)));
    let _ = app.update(Message::Perform(PerformMsg::Clips(ClipMsg::LaunchRow(0))));
    assert_eq!(
        launches(&mut sent),
        vec![(MusicalBoundary::Eighth, vec![breaks, pads])]
    );
}

#[tokio::test]
async fn a_clip_launch_quantization_survives_duplicate_save_and_reopen() {
    let breaks = TrackId::new();
    let (mut app, _sent) = clip_app(&[breaks]);
    let clip = app.state.perform.clips.at(breaks, 0).unwrap().id;
    let _ = app.update(set_clip(clip, Some(MusicalBoundary::Sixteenth)));
    let _ = app.update(Message::Perform(PerformMsg::Clips(ClipMsg::Duplicate(
        clip,
    ))));
    let copies: Vec<_> = app
        .state
        .perform
        .clips
        .clips
        .iter()
        .map(|clip| (clip.id, clip.launch_quantization))
        .collect();
    assert_eq!(copies.len(), 2);
    assert!(copies
        .iter()
        .all(|(_, q)| *q == Some(MusicalBoundary::Sixteenth)));
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("jungle.vzp");
    super::save_project_async(path.clone(), None, app.project_from_state())
        .await
        .unwrap();
    let loaded = super::load_project_async(path, None).await.unwrap();
    let mut reopened = super::test_support::app();
    reopened.rebuild_from_loaded_project(loaded);
    for (id, quantization) in copies {
        assert_eq!(
            reopened
                .state
                .perform
                .clips
                .by_id(id)
                .unwrap()
                .launch_quantization,
            quantization
        );
    }
}

#[test]
fn stopping_a_playing_clip_waits_for_its_own_quantization() {
    let breaks = TrackId::new();
    let (mut app, mut sent) = clip_app(&[breaks]);
    let clip = app.state.perform.clips.at(breaks, 0).unwrap().clone();
    let _ = app.update(set_clip(clip.id, Some(MusicalBoundary::Eighth)));
    for stop in [ClipMsg::Toggle(clip.id), ClipMsg::StopTrack(breaks)] {
        let editor = &mut app.state.perform.clip_editor;
        editor.running = true;
        editor.queued.clear();
        editor.playing.insert(breaks, clip.clone());
        let _ = app.update(Message::Perform(PerformMsg::Clips(stop)));
        assert_eq!(
            launches(&mut sent),
            vec![(MusicalBoundary::Eighth, vec![breaks])]
        );
    }
}

#[test]
fn choosing_a_clips_current_quantization_is_not_an_edit() {
    let breaks = TrackId::new();
    let (mut app, _sent) = clip_app(&[breaks]);
    let clip = app.state.perform.clips.at(breaks, 0).unwrap().id;
    let _ = app.update(set_clip(clip, None));
    assert!(!app.state.project.dirty);
    assert!(!app.state.project.history.can_undo());
}
