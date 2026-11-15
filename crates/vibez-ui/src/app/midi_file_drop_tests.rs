//! MIDI files dropped from the Browser land as note Clips, through the router
//! and a real file read.

use std::path::{Path, PathBuf};

use midly::num::{u15, u28, u4, u7};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};
use vibez_core::id::TrackId;
use vibez_core::midi::{InstrumentKind, TrackKind};
use vibez_core::track::MediaSourceRef;

use super::test_support::{app, run_until_idle};
use super::*;
use crate::domains::midi_file_import::MidiFileLayout;
use crate::domains::project::ProjectMsg;
use crate::state::ProjectTrack;

const PPQ: u16 = 96;

fn event(delta: u32, kind: TrackEventKind<'static>) -> TrackEvent<'static> {
    TrackEvent {
        delta: u28::new(delta),
        kind,
    }
}

/// A one-step General MIDI drum hit `start` ticks after the previous event.
fn hit(start: u32, key: u8) -> [TrackEvent<'static>; 2] {
    let note = |delta, vel| {
        event(
            delta,
            TrackEventKind::Midi {
                channel: u4::new(9),
                message: MidiMessage::NoteOn {
                    key: u7::new(key),
                    vel: u7::new(vel),
                },
            },
        )
    };
    [note(start, 110), note(u32::from(PPQ) / 4, 0)]
}

fn track_name(name: &'static str) -> TrackEvent<'static> {
    event(
        0,
        TrackEventKind::Meta(MetaMessage::TrackName(name.as_bytes())),
    )
}

fn end_of_track(delta: u32) -> TrackEvent<'static> {
    event(delta, TrackEventKind::Meta(MetaMessage::EndOfTrack))
}

fn write_midi(path: PathBuf, format: Format, tracks: Vec<Vec<TrackEvent<'static>>>) -> PathBuf {
    let mut smf = Smf::new(Header::new(format, Timing::Metrical(u15::new(PPQ))));
    smf.tracks = tracks;
    smf.save(&path).unwrap();
    path
}

/// A one-bar type 0 drum phrase: kick on 1, snare on 2.
fn drum_phrase(directory: &Path) -> PathBuf {
    let mut events = Vec::new();
    events.extend(hit(0, 36));
    events.extend(hit(u32::from(PPQ) * 3 / 4, 38));
    events.push(end_of_track(u32::from(PPQ) * 11 / 4));
    write_midi(
        directory.join("Rebeater P3.mid"),
        Format::SingleTrack,
        vec![events],
    )
}

/// The same bar split into a named track per drum.
fn split_phrase(directory: &Path) -> PathBuf {
    let mut kick = vec![track_name("Kick")];
    kick.extend(hit(0, 36));
    kick.push(end_of_track(u32::from(PPQ) * 15 / 4));
    let mut hats = vec![track_name("Closed Hat")];
    hats.extend(hit(u32::from(PPQ) / 2, 42));
    hats.push(end_of_track(u32::from(PPQ) * 13 / 4));
    write_midi(
        directory.join("Rebeater P3 split.mid"),
        Format::Parallel,
        vec![kick, hats],
    )
}

fn app_with_track(kind: TrackKind) -> (App, TrackId) {
    let mut app = app();
    let id = TrackId::new();
    let track = if kind.is_midi() {
        ProjectTrack::new_instrument(id, "MIDI 1".into(), kind, 0)
    } else {
        ProjectTrack::new(id, "Audio 1".into(), 0)
    };
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(track);
    (app, id)
}

fn drag(app: &mut App, path: PathBuf) {
    app.state.browser.drag_label = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    app.state.browser.drag_source = Some(MediaSourceRef::LocalFile { path });
}

async fn drop_on_lane(app: &mut App, track_id: TrackId, beat: f64, path: PathBuf) {
    drag(app, path);
    let position_samples = app.state.transport.beats_to_samples(beat);
    run_until_idle(
        app,
        Message::DropSampleOnArrangement {
            track_id,
            position_samples,
        },
    )
    .await;
}

fn note_clips(app: &App, track_id: TrackId) -> Vec<crate::state::UiNoteClip> {
    app.state
        .arrangement
        .timeline
        .get(track_id)
        .map(|content| content.note_clips.clone())
        .unwrap_or_default()
}

fn pitches(clip: &crate::state::UiNoteClip) -> Vec<(f64, u8)> {
    clip.notes
        .iter()
        .map(|note| (note.start_beat, note.pitch))
        .collect()
}

fn new_tracks(app: &App, existing: TrackId) -> Vec<&ProjectTrack> {
    app.state
        .project_tracks
        .tracks
        .iter()
        .filter(|track| track.id != existing)
        .collect()
}

#[tokio::test]
async fn a_midi_file_dropped_on_a_midi_lane_lands_as_one_clip_there_in_one_undo_step() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, track_id) = app_with_track(TrackKind::Midi);

    drop_on_lane(&mut app, track_id, 8.0, drum_phrase(directory.path())).await;

    let clips = note_clips(&app, track_id);
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].name, "Rebeater P3");
    assert_eq!(clips[0].position_beats, 8.0);
    assert_eq!(clips[0].duration_beats, 4.0);
    assert_eq!(pitches(&clips[0]), vec![(0.0, 36), (1.0, 38)]);
    assert!(app.state.browser.drag_source.is_none());
    assert!(app.state.project.dirty);

    let _ = app.update(Message::Project(ProjectMsg::Undo));
    assert!(note_clips(&app, track_id).is_empty());
    assert!(app.state.project.history.undo.is_empty());
}

#[tokio::test]
async fn drums_dropped_on_empty_arrange_get_a_new_drum_rack_track_named_for_the_file() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, existing) = app_with_track(TrackKind::Audio);
    drag(&mut app, drum_phrase(directory.path()));

    run_until_idle(&mut app, Message::DropSampleOnEmptyArrangement).await;

    let added = new_tracks(&app, existing);
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].name, "Rebeater P3");
    assert!(added[0].kind.is_midi());
    assert_eq!(added[0].instrument_kind, Some(InstrumentKind::DrumRack));
    assert_eq!(note_clips(&app, added[0].id).len(), 1);

    let _ = app.update(Message::Project(ProjectMsg::Undo));
    assert!(new_tracks(&app, existing).is_empty());
}

#[tokio::test]
async fn an_audio_lane_turns_a_midi_file_away() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, track_id) = app_with_track(TrackKind::Audio);

    drop_on_lane(&mut app, track_id, 0.0, drum_phrase(directory.path())).await;

    assert!(note_clips(&app, track_id).is_empty());
    assert!(app.state.project.history.undo.is_empty());
    assert!(app.state.status_text.contains("MIDI track"));
}

#[tokio::test]
async fn a_device_panel_turns_a_midi_file_away() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, track_id) = app_with_track(TrackKind::Midi);
    drag(&mut app, drum_phrase(directory.path()));

    run_until_idle(&mut app, Message::DropSampleOnDevicePanel { track_id }).await;

    let track = app.state.find_track(track_id).unwrap();
    assert_eq!(track.instrument_kind, None);
    assert!(app.state.project.history.undo.is_empty());
    assert!(app.state.status_text.contains("MIDI track"));
}

#[tokio::test]
async fn a_multi_track_file_waits_for_the_producers_layout_and_cancel_leaves_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, track_id) = app_with_track(TrackKind::Midi);

    drop_on_lane(&mut app, track_id, 4.0, split_phrase(directory.path())).await;

    assert!(app.state.view.midi_file_layout_choice.is_some());
    assert!(note_clips(&app, track_id).is_empty());

    let _ = app.update(Message::ChooseMidiFileLayout(None));
    assert!(app.state.view.midi_file_layout_choice.is_none());
    assert!(note_clips(&app, track_id).is_empty());
    assert!(app.state.project.history.undo.is_empty());
}

#[tokio::test]
async fn one_clip_merges_a_multi_track_file_onto_the_dropped_lane() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, track_id) = app_with_track(TrackKind::Midi);
    drop_on_lane(&mut app, track_id, 4.0, split_phrase(directory.path())).await;

    let _ = app.update(Message::ChooseMidiFileLayout(Some(MidiFileLayout::OneClip)));

    let clips = note_clips(&app, track_id);
    assert_eq!(clips.len(), 1);
    assert_eq!(pitches(&clips[0]), vec![(0.0, 36), (0.5, 42)]);
    assert_eq!(clips[0].position_beats, 4.0);
    assert!(new_tracks(&app, track_id).is_empty());
    assert!(app.state.view.midi_file_layout_choice.is_none());
}

#[tokio::test]
async fn a_track_per_file_track_adds_named_drum_rack_tracks_in_one_undo_step() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, track_id) = app_with_track(TrackKind::Midi);
    drop_on_lane(&mut app, track_id, 4.0, split_phrase(directory.path())).await;

    let _ = app.update(Message::ChooseMidiFileLayout(Some(
        MidiFileLayout::TrackPerFileTrack,
    )));

    let added = new_tracks(&app, track_id);
    let names: Vec<_> = added.iter().map(|track| track.name.as_str()).collect();
    assert_eq!(names, vec!["Kick", "Closed Hat"]);
    assert!(added
        .iter()
        .all(|track| track.instrument_kind == Some(InstrumentKind::DrumRack)));
    let hats = note_clips(&app, added[1].id);
    assert_eq!(pitches(&hats[0]), vec![(0.5, 42)]);
    assert_eq!(hats[0].position_beats, 4.0);
    assert!(note_clips(&app, track_id).is_empty());

    let _ = app.update(Message::Project(ProjectMsg::Undo));
    assert!(new_tracks(&app, track_id).is_empty());
    assert!(app.state.project.history.undo.is_empty());
}

#[tokio::test]
async fn a_midi_file_dropped_on_a_clip_slot_becomes_that_slots_clip() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, track_id) = app_with_track(TrackKind::Midi);
    app.state.perform.layout = vibez_project::PerformLayout::Clips;
    drag(&mut app, drum_phrase(directory.path()));

    run_until_idle(&mut app, Message::ImportLauncherClip { track_id, row: 2 }).await;

    let clip = app.state.perform.clips.at(track_id, 2).expect("slot clip");
    assert_eq!(clip.name(), "Rebeater P3");
    let content = clip.timeline.get(track_id).unwrap();
    assert_eq!(pitches(&content.note_clips[0]), vec![(0.0, 36), (1.0, 38)]);
    assert_eq!(content.note_clips[0].position_beats, 0.0);
}

#[tokio::test]
async fn choosing_a_midi_file_in_the_browser_selects_it_without_auditioning() {
    let directory = tempfile::tempdir().unwrap();
    let source = MediaSourceRef::LocalFile {
        path: drum_phrase(directory.path()),
    };
    for enabled in [true, false] {
        let mut app = app();
        app.state.browser.audition_enabled = enabled;

        run_until_idle(&mut app, Message::ClickLocalBrowserEntry(source.clone())).await;
        run_until_idle(&mut app, Message::PreviewLocalEntry(source.clone())).await;

        let browser = &app.state.browser;
        assert_eq!(browser.selected_source.as_ref(), Some(&source));
        assert!(!browser.waveform_loading && !browser.audition_loading);
        assert_eq!(browser.waveform_error, None);
        assert_eq!(
            app.state.status_text,
            "Drag Rebeater P3.mid onto a MIDI track, empty Arrange or a MIDI track's Clip slot"
        );
    }
}
