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
use crate::message::BrowserImportTarget;
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
pub(super) fn drum_phrase(directory: &Path) -> PathBuf {
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
async fn choosing_a_midi_file_in_the_browser_says_where_it_can_be_dragged() {
    let directory = tempfile::tempdir().unwrap();
    let source = MediaSourceRef::LocalFile {
        path: drum_phrase(directory.path()),
    };
    let mut app = app();
    app.state.browser.audition_enabled = false;

    run_until_idle(&mut app, Message::ClickLocalBrowserEntry(source.clone())).await;

    let browser = &app.state.browser;
    assert_eq!(browser.selected_source.as_ref(), Some(&source));
    assert!(!browser.waveform_loading && !browser.audition_loading);
    assert_eq!(browser.waveform_error, None);
    assert_eq!(
        app.state.status_text,
        "Drag Rebeater P3.mid onto a MIDI track, empty Arrange or a MIDI track's Clip slot"
    );
}

#[tokio::test]
async fn escape_cancels_a_multi_track_file_waiting_for_its_layout() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, track_id) = app_with_track(TrackKind::Midi);
    drop_on_lane(&mut app, track_id, 0.0, split_phrase(directory.path())).await;

    super::test_support::deliver_key(
        &mut app,
        super::test_support::escape_key(),
        iced::event::Status::Ignored,
    );

    assert!(app.state.view.midi_file_layout_choice.is_none());
    assert!(note_clips(&app, track_id).is_empty());
    assert!(app.state.project.history.undo.is_empty());
}

#[tokio::test]
async fn importing_the_selected_midi_file_lands_on_the_selected_midi_track_at_the_playhead() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, track_id) = app_with_track(TrackKind::Midi);
    app.state.arrangement.selected_track = Some(track_id);
    app.state.browser.selected_source = Some(MediaSourceRef::LocalFile {
        path: drum_phrase(directory.path()),
    });
    app.state.transport.position_samples = app.state.transport.beats_to_samples(2.0);

    run_until_idle(&mut app, Message::ImportSelectedBrowserSampleToArrangement).await;

    let clips = note_clips(&app, track_id);
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].position_beats, 2.0);
    assert_eq!(app.state.project_tracks.tracks.len(), 1);
}

fn record_engine(app: &mut App) -> rtrb::Consumer<vibez_engine::commands::EngineCommand> {
    let (producer, commands) = rtrb::RingBuffer::new(4096);
    app.cmd_tx = crate::domains::EngineCommandQueue::new(
        producer,
        vibez_engine::transport_ring::transport_ring().0,
    );
    commands
}

fn sent(
    commands: &mut rtrb::Consumer<vibez_engine::commands::EngineCommand>,
) -> Vec<vibez_engine::commands::EngineCommand> {
    std::iter::from_fn(|| commands.pop().ok()).collect()
}

#[tokio::test]
async fn a_clip_on_an_existing_lane_reaches_the_engine_with_its_notes() {
    use vibez_engine::commands::EngineCommand;
    let directory = tempfile::tempdir().unwrap();
    let (mut app, track_id) = app_with_track(TrackKind::Midi);
    let mut commands = record_engine(&mut app);

    drop_on_lane(&mut app, track_id, 0.0, drum_phrase(directory.path())).await;

    assert!(sent(&mut commands).iter().any(|command| matches!(
        command,
        EngineCommand::AddNoteClip { track_id: lane, .. } if *lane == track_id
    )));
}

#[tokio::test]
async fn a_new_track_reaches_the_engine_with_its_drum_rack_and_clip() {
    use vibez_engine::commands::EngineCommand;
    let directory = tempfile::tempdir().unwrap();
    let mut app = app();
    let mut commands = record_engine(&mut app);
    drag(&mut app, drum_phrase(directory.path()));

    run_until_idle(&mut app, Message::DropSampleOnEmptyArrangement).await;

    let track_id = app.state.project_tracks.tracks[0].id;
    let sent = sent(&mut commands);
    let position = |wanted: fn(&EngineCommand, TrackId) -> bool| {
        sent.iter()
            .position(|command| wanted(command, track_id))
            .expect("command sent")
    };
    let added = position(|c, t| matches!(c, EngineCommand::AddTrack(info) if info.id == t));
    let instrument = position(
        |c, t| matches!(c, EngineCommand::SetInstrument { track_id, .. } if *track_id == t),
    );
    let clip =
        position(|c, t| matches!(c, EngineCommand::AddNoteClip { track_id, .. } if *track_id == t));
    assert!(added < instrument && added < clip);
}

#[tokio::test]
async fn a_midi_file_dropped_on_a_section_lane_lands_in_that_section() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, track_id) = app_with_track(TrackKind::Midi);
    let section = crate::domains::perform::Section::new(0);
    let section_id = section.id;
    Arc::make_mut(&mut app.state.perform.sections).insert(section);
    app.state.view.workspace = crate::state::Workspace::Perform;
    let _ = app.update(Message::Perform(
        crate::domains::perform::PerformMsg::SelectSection(section_id),
    ));

    drop_on_lane(&mut app, track_id, 4.0, drum_phrase(directory.path())).await;

    let section = app.state.perform.sections.by_id(section_id).unwrap();
    let clips = &section.timeline.get(track_id).unwrap().note_clips;
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].position_beats, 4.0);
    assert!(note_clips(&app, track_id).is_empty());
}

#[tokio::test]
async fn a_section_deleted_while_its_file_is_read_takes_nothing() {
    let (mut app, track_id) = app_with_track(TrackKind::Midi);
    let import = crate::domains::midi_file_import::MidiFileImport {
        name: "Rebeater P3".into(),
        file: vibez_core::midi_file::MidiFile {
            tracks: vec![vibez_core::midi_file::MidiFileTrack {
                name: None,
                notes: vec![vibez_core::midi::MidiNote {
                    pitch: 36,
                    velocity: 100,
                    start_beat: 0.0,
                    duration_beats: 0.25,
                }],
                on_drum_channel: true,
            }],
            length_beats: 4.0,
        },
    };
    let generation = app.browser_import_request.begin();

    let _ = app.update(Message::MidiFileRead {
        target: BrowserImportTarget::SectionClipAt {
            section_id: vibez_core::id::SectionId::new(),
            track_id,
            position_samples: 0,
        },
        generation,
        result: Ok(import),
    });

    assert!(app.state.project.history.undo.is_empty());
    assert!(!app.state.project.dirty);
    assert!(!app.state.status_text.starts_with("Imported"));
}

#[tokio::test]
async fn a_read_no_drop_waits_for_any_more_lands_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, track_id) = app_with_track(TrackKind::Midi);
    let stale = app.browser_import_request.begin();
    app.browser_import_request.cancel();

    drag(&mut app, drum_phrase(directory.path()));
    let bytes = std::fs::read(app_drag_path(&app)).unwrap();
    let _ = app.update(Message::MidiFileRead {
        target: BrowserImportTarget::ArrangementClipAt {
            track_id,
            position_samples: 0,
        },
        generation: stale,
        result: Ok(crate::domains::midi_file_import::MidiFileImport {
            name: "Rebeater P3".into(),
            file: vibez_core::midi_file::MidiFile::read(&bytes).unwrap(),
        }),
    });

    assert!(note_clips(&app, track_id).is_empty());
    assert!(app.state.project.history.undo.is_empty());
}

fn app_drag_path(app: &App) -> PathBuf {
    match app.state.browser.drag_source.as_ref() {
        Some(MediaSourceRef::LocalFile { path }) => path.clone(),
        _ => unreachable!("the test drags a local file"),
    }
}

#[tokio::test]
async fn new_tracks_exist_in_the_engine_before_a_playing_section_refreshes() {
    use vibez_engine::commands::EngineCommand;
    let directory = tempfile::tempdir().unwrap();
    let (mut app, track_id) = app_with_track(TrackKind::Midi);
    let section = crate::domains::perform::Section::new(0);
    let section_id = section.id;
    Arc::make_mut(&mut app.state.perform.sections).insert(section);
    app.state.view.workspace = crate::state::Workspace::Perform;
    let _ = app.update(Message::Perform(
        crate::domains::perform::PerformMsg::SelectSection(section_id),
    ));
    app.state.perform.playing_section = Some(section_id);
    drop_on_lane(&mut app, track_id, 0.0, split_phrase(directory.path())).await;
    let mut commands = record_engine(&mut app);

    let _ = app.update(Message::ChooseMidiFileLayout(Some(
        MidiFileLayout::TrackPerFileTrack,
    )));

    let sent = sent(&mut commands);
    let refresh = sent
        .iter()
        .rposition(|command| matches!(command, EngineCommand::RefreshSection(_)))
        .expect("the playing Section refreshes");
    for track in new_tracks(&app, track_id) {
        let added = sent
            .iter()
            .position(
                |command| matches!(command, EngineCommand::AddTrack(info) if info.id == track.id),
            )
            .expect("the new track reaches the engine");
        assert!(
            added < refresh,
            "{} is added before the refresh",
            track.name
        );
    }
}

#[tokio::test]
async fn a_track_each_on_a_clip_slot_fills_that_row_and_selects_the_first_track_and_clip() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, track_id) = app_with_track(TrackKind::Midi);
    app.state.perform.layout = vibez_project::PerformLayout::Clips;
    drag(&mut app, split_phrase(directory.path()));
    run_until_idle(&mut app, Message::ImportLauncherClip { track_id, row: 1 }).await;

    let _ = app.update(Message::ChooseMidiFileLayout(Some(
        MidiFileLayout::TrackPerFileTrack,
    )));

    let added: Vec<_> = new_tracks(&app, track_id).iter().map(|t| t.id).collect();
    assert_eq!(added.len(), 2);
    let first = app
        .state
        .perform
        .clips
        .at(added[0], 1)
        .expect("first slot")
        .id;
    assert!(app.state.perform.clips.at(added[1], 1).is_some());
    assert_eq!(app.state.arrangement.selected_track, Some(added[0]));
    assert_eq!(app.state.perform.clip_editor.selected, Some(first));
}

#[tokio::test]
async fn a_project_reset_closes_a_waiting_layout_choice() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, track_id) = app_with_track(TrackKind::Midi);
    drop_on_lane(&mut app, track_id, 0.0, split_phrase(directory.path())).await;

    app.reset_to_new_project();

    assert!(app.state.view.midi_file_layout_choice.is_none());
}
