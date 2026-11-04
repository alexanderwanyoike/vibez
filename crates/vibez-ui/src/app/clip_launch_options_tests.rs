//! Each Project Track, and each Clip, launches with its own options.

use std::sync::Arc;

use vibez_core::id::{ClipId, TrackId};
use vibez_core::perform::{ClipLaunchMode, MusicalBoundary};
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
            launch_mode: ClipLaunchMode::Play,
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

fn set_mode(track_id: TrackId, mode: ClipLaunchMode) -> Message {
    Message::Perform(PerformMsg::Clips(ClipMsg::SetLaunchMode { track_id, mode }))
}

#[test]
fn a_legato_track_launches_its_clips_in_legato() {
    let breaks = TrackId::new();
    let pads = TrackId::new();
    let (mut app, mut sent) = clip_app(&[breaks, pads]);
    let _ = app.update(set_mode(breaks, ClipLaunchMode::Legato));
    assert!(app.state.project.dirty);
    let _ = app.update(Message::Perform(PerformMsg::Clips(ClipMsg::LaunchRow(0))));
    let modes: Vec<_> = std::iter::from_fn(|| sent.pop().ok())
        .filter_map(|command| match command {
            EngineCommand::QueueClips { clips, .. } => Some(clips),
            _ => None,
        })
        .flatten()
        .map(|clip| (clip.track_id, clip.launch_mode))
        .collect();
    assert_eq!(
        modes,
        vec![
            (breaks, ClipLaunchMode::Legato),
            (pads, ClipLaunchMode::Play)
        ]
    );
}

#[tokio::test]
async fn track_launch_mode_reopens_with_the_project() {
    let breaks = TrackId::new();
    let pads = TrackId::new();
    let (mut app, _sent) = clip_app(&[breaks, pads]);
    let _ = app.update(set_mode(breaks, ClipLaunchMode::Legato));
    let project = app.project_from_state();
    assert_eq!(
        project.launcher_tracks,
        vec![vibez_project::LauncherTrackInfo {
            track_id: breaks,
            launch_quantization: MusicalBoundary::OneBar,
            launch_mode: ClipLaunchMode::Legato,
        }]
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
    assert_eq!(clips.launch_mode(breaks), ClipLaunchMode::Legato);
    assert_eq!(clips.launch_mode(pads), ClipLaunchMode::Play);
}

#[test]
fn undo_restores_the_previous_track_launch_mode() {
    let breaks = TrackId::new();
    let (mut app, _sent) = clip_app(&[breaks]);
    let _ = app.update(set_mode(breaks, ClipLaunchMode::Legato));
    let _ = app.update(Message::Project(crate::domains::project::ProjectMsg::Undo));
    assert_eq!(
        app.state.perform.clips.launch_mode(breaks),
        ClipLaunchMode::Play
    );
}

#[test]
fn a_legato_transition_plays_and_captures_from_its_start_position() {
    let breaks = TrackId::new();
    let (mut app, _sent) = clip_app(&[breaks]);
    let (mut events, receiver) = rtrb::RingBuffer::new(8);
    app.event_rx = Some(receiver);
    app.state.transport.sample_rate = 8;
    app.state.transport.bpm = 120.0;
    let mut clip = app.state.perform.clips.at(breaks, 0).unwrap().clone();
    let notes = &mut Arc::make_mut(&mut clip.timeline).ensure(breaks).note_clips[0].notes;
    for (pitch, start_beat) in [(42, 0.5), (43, 3.0)] {
        notes.push(vibez_core::midi::MidiNote {
            pitch,
            velocity: 100,
            start_beat,
            duration_beats: 0.5,
        });
    }
    let perform = &mut app.state.perform;
    perform.clip_editor.pending.insert(1, clip.clone());
    perform.clip_editor.next_request = 1;
    perform.capture.phase = crate::domains::perform::CapturePhase::Starting;
    perform.capture.prepare(0, 8, 120.0);
    perform.capture.start(0, None);
    events
        .push(vibez_engine::events::EngineEvent::ClipTransitioned {
            track_id: breaks,
            clip_id: Some(clip.id),
            request_id: 1,
            effective_at_samples: 8,
            start_position: 8,
            retired: None,
        })
        .unwrap();
    app.poll_engine_events();
    let perform = &mut app.state.perform;
    assert_eq!(
        perform.clip_editor.playhead_samples(clip.id, 9, 4.0),
        Some(9)
    );
    let captured = perform.capture.finish(16).unwrap().materialize();
    let pitches: Vec<_> = captured.by_track[&breaks].note_clips[0]
        .notes
        .iter()
        .map(|note| note.pitch)
        .collect();
    assert_eq!(pitches, vec![43]);
}

#[test]
fn choosing_a_tracks_current_launch_mode_is_not_an_edit() {
    let breaks = TrackId::new();
    let (mut app, _sent) = clip_app(&[breaks]);
    let _ = app.update(set_mode(breaks, ClipLaunchMode::Play));
    assert!(!app.state.project.dirty);
    assert!(!app.state.project.history.can_undo());
}
