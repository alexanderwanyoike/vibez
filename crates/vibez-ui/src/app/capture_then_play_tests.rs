//! Play right after a Capture, or right after undoing one, starts in the next callback.

use super::*;
use crate::domains::perform::capture::{CaptureAction, CaptureMsg, CapturedTimelineSource};
use crate::domains::perform::Section;
use crate::domains::project::ProjectMsg;
use crate::state::{ProjectTrack, UiNoteClip};
use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::MidiNote;
use vibez_core::perform::GrooveGrid;
use vibez_engine::engine::{AudioEngine, AudioProcessBlock};

const SAMPLES_PER_BEAT: u64 = 22_050;

fn note_clip(position_beats: f64) -> UiNoteClip {
    UiNoteClip {
        id: ClipId::new(),
        name: "Groove".into(),
        position_beats,
        duration_beats: 4.0,
        notes: (0..8)
            .map(|step| MidiNote {
                pitch: 36,
                velocity: 100,
                start_beat: f64::from(step) * 0.5,
                duration_beats: 0.25,
            })
            .collect(),
        selected_notes: Default::default(),
        start_marker_beats: 0.0,
        loop_enabled: false,
        loop_start_beats: 0.0,
        loop_end_beats: 4.0,
        groove_grid: GrooveGrid::Off,
    }
}

fn callback(engine: &mut AudioEngine) {
    engine.process_block(AudioProcessBlock::new(&mut [0.0; 512], 2));
}

/// Two tracks, the first with arranged notes, both running in a real engine.
fn project() -> (App, AudioEngine, [TrackId; 2]) {
    let mut app = super::test_support::app();
    let (mut engine, commands, events) = AudioEngine::new();
    app.cmd_tx =
        crate::domains::EngineCommandQueue::new(commands, engine.take_transport_sender().unwrap());
    app.event_rx = Some(events);
    let tracks = [TrackId::new(), TrackId::new()];
    for (index, track_id) in tracks.into_iter().enumerate() {
        let track = ProjectTrack::new(track_id, format!("Track {index}"), index as u8);
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(track);
    }
    Arc::make_mut(&mut app.state.arrangement.timeline)
        .ensure(tracks[0])
        .note_clips
        .extend([8.0, 12.0, 16.0, 20.0].map(note_clip));
    for track in app.state.project_tracks.tracks.clone() {
        app.replay_track_to_engine(&track);
    }
    settle(&mut app, &mut engine);
    (app, engine, tracks)
}

/// Serves every storage request the way the UI tick does until the engine
/// has taken all queued content.
fn settle(app: &mut App, engine: &mut AudioEngine) {
    for _ in 0..256 {
        callback(engine);
        app.poll_engine_events();
    }
}

/// A Capture of one Section pass in which the user muted the second track.
fn capture_with_a_perform_mute(app: &mut App, tracks: [TrackId; 2]) {
    let mut section = Section::new(0);
    section.length_beats = 4.0;
    for track_id in tracks {
        Arc::make_mut(&mut section.timeline)
            .ensure(track_id)
            .note_clips
            .push(note_clip(0.0));
    }
    let _ = app.state.perform.capture.update(CaptureMsg::Toggle);
    let _ = app.apply_capture_action(CaptureAction::Start);
    let capture = &mut app.state.perform.capture;
    capture.start(0, Some((CapturedTimelineSource::from_section(&section), 0)));
    capture.track_mute_changed(tracks[1], true, SAMPLES_PER_BEAT);
    let completed = capture.finish(4 * SAMPLES_PER_BEAT);
    app.finish_performance_capture(completed);
}

#[test]
fn play_right_after_a_capture_with_perform_mutes_starts_in_the_next_callback() {
    let (mut app, mut engine, tracks) = project();

    capture_with_a_perform_mute(&mut app, tracks);
    app.send_command(EngineCommand::Play);
    callback(&mut engine);

    assert!(
        engine.transport().is_playing(),
        "Play must not wait for the Capture's clips to reach the engine"
    );
}

#[test]
fn play_right_after_undoing_a_capture_starts_in_the_next_callback() {
    let (mut app, mut engine, tracks) = project();
    capture_with_a_perform_mute(&mut app, tracks);
    settle(&mut app, &mut engine);

    let _ = app.update_and_refresh_clips(Message::Project(ProjectMsg::Undo));
    app.send_command(EngineCommand::Play);
    callback(&mut engine);

    assert!(
        engine.transport().is_playing(),
        "Play must not wait for the undone project to reach the engine"
    );
}
