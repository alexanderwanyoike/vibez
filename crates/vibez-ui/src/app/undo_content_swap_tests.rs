//! Undo of a content edit swaps only the clips that changed, leaving every track and device running.

use super::*;
use crate::domains::arrangement::ArrangementMsg;
use crate::domains::project::ProjectMsg;
use crate::state::{ArrangementSelection, ProjectTrack, UiEffect, UiNoteClip, UndoGestureId};
use vibez_core::effect::{EffectType, PluginDeviceInfo};
use vibez_core::id::{ClipId, EffectId, TrackId};
use vibez_core::midi::{MidiNote, TrackKind};

fn note_clip(position_beats: f64) -> UiNoteClip {
    UiNoteClip {
        name: "Pattern".into(),
        notes: vec![MidiNote {
            pitch: 60,
            velocity: 100,
            start_beat: 0.0,
            duration_beats: 1.0,
        }],
        ..UiNoteClip::test_clip(position_beats, 4.0)
    }
}

fn plugin_effect() -> UiEffect {
    let device = PluginDeviceInfo {
        format: "clap".into(),
        uid: "test.reverb".into(),
        path: "/test.clap".into(),
        name: "Reverb".into(),
        state_b64: None,
    };
    UiEffect {
        reconfiguration_failed: false,
        latency_samples: None,
        id: EffectId::new(),
        effect_type: EffectType::Gain,
        bypass: false,
        params: vec![],
        descriptors: &[],
        plugin_name: Some(device.name.clone()),
        has_plugin_gui: false,
        plugin_ref: Some(device),
        inactive_sidechains: Default::default(),
        sidechains: vec![],
        external_inputs: vec![],
    }
}

/// Two MIDI tracks with one clip each; the first also hosts a plugin.
fn two_track_project() -> (App, rtrb::Consumer<EngineCommand>, TrackId, ClipId) {
    let mut app = super::test_support::app();
    let (producer, commands) = rtrb::RingBuffer::new(4096);
    app.cmd_tx = crate::domains::EngineCommandQueue::new(
        producer,
        vibez_engine::transport_ring::transport_ring().0,
    );
    let mut moved = None;
    for (index, name) in ["Keys", "Drums"].into_iter().enumerate() {
        let mut track = ProjectTrack::new(TrackId::new(), name.into(), index as u8);
        track.kind = TrackKind::Midi;
        if index == 0 {
            track.effects.push(plugin_effect());
        }
        let clip = note_clip(0.0);
        moved.get_or_insert((track.id, clip.id));
        Arc::make_mut(&mut app.state.arrangement.timeline)
            .ensure(track.id)
            .note_clips
            .push(clip);
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(track);
    }
    let (track_id, clip_id) = moved.unwrap();
    (app, commands, track_id, clip_id)
}

fn drain(commands: &mut rtrb::Consumer<EngineCommand>) -> Vec<EngineCommand> {
    std::iter::from_fn(|| commands.pop().ok()).collect()
}

#[test]
fn undoing_a_clip_move_swaps_only_that_clip() {
    let (mut app, mut commands, track_id, clip_id) = two_track_project();
    let _ = app.update_and_refresh_clips(Message::Arrangement(
        ArrangementMsg::SelectArrangementClip {
            selection: ArrangementSelection::NoteClip { track_id, clip_id },
            shift_held: false,
        },
    ));
    let _ = app.update_and_refresh_clips(Message::UndoGesture {
        id: UndoGestureId::new(),
        edit: Box::new(Message::Arrangement(ArrangementMsg::MoveSelectedClips {
            anchor: clip_id,
            new_position_beats: 8.0,
            target_track: track_id,
        })),
    });
    assert_eq!(
        app.state.arrange_content(track_id).unwrap().note_clips[0].position_beats,
        8.0
    );
    drain(&mut commands);

    let _ = app.update_and_refresh_clips(Message::Project(ProjectMsg::Undo));

    let sent = drain(&mut commands);
    assert!(
        !sent.iter().any(|command| matches!(
            command,
            EngineCommand::RemoveTrack(_)
                | EngineCommand::AddTrack(_)
                | EngineCommand::RemoveBus(_)
                | EngineCommand::RemoveEffect(..)
                | EngineCommand::InsertEffect { .. }
                | EngineCommand::SetInstrument { .. }
        )),
        "tracks and devices that did not change stay in the engine"
    );
    assert!(
        !app.state.status_text.contains("plugin"),
        "no plugin reloads: {}",
        app.state.status_text
    );
    let clip_edits: Vec<_> = sent
        .iter()
        .filter(|command| {
            matches!(
                command,
                EngineCommand::AddClip { .. }
                    | EngineCommand::RemoveClip(..)
                    | EngineCommand::AddNoteClip { .. }
                    | EngineCommand::RemoveNoteClip(..)
                    | EngineCommand::SetAutomationLane { .. }
                    | EngineCommand::RemoveAutomationLane { .. }
            )
        })
        .collect();
    assert!(
        matches!(
            clip_edits.as_slice(),
            [
                EngineCommand::RemoveNoteClip(removed_track, removed_clip),
                EngineCommand::AddNoteClip { track_id: added_track, .. },
            ] if *removed_track == track_id && *removed_clip == clip_id && *added_track == track_id
        ),
        "one swap of the moved clip"
    );
    assert_eq!(
        app.state.arrange_content(track_id).unwrap().note_clips[0].position_beats,
        0.0
    );
}
