//! A file dropped on Perform's empty space gets a new track for its Clip.

use std::path::{Path, PathBuf};

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::{SectionId, TrackId};
use vibez_core::midi::{InstrumentKind, TrackKind};
use vibez_core::track::MediaSourceRef;

use super::test_support::{app, run_until_idle};
use super::*;
use crate::domains::browser::BrowserMsg;
use crate::domains::drop_ghost::{GhostPlace, NewTrack};
use crate::state::{AuditionMode, ProjectTrack};

/// A one-bar type 0 drum phrase at 96 PPQ: kick on 1, snare on 2.
pub(super) fn drum_phrase(directory: &Path) -> PathBuf {
    let bytes: &[u8] = &[
        b'M', b'T', b'h', b'd', 0, 0, 0, 6, 0, 0, 0, 1, 0, 96, //
        b'M', b'T', b'r', b'k', 0, 0, 0, 20, //
        0, 0x99, 36, 110, 24, 0x89, 36, 0, //
        72, 0x99, 38, 110, 24, 0x89, 38, 0, //
        0x82, 8, 0xff, 0x2f, 0x00,
    ];
    let path = directory.join("Rebeater P3.mid");
    std::fs::write(&path, bytes).unwrap();
    path
}

pub(super) fn kick(directory: &Path) -> PathBuf {
    let path = directory.join("kick.wav");
    let audio = DecodedAudio {
        channels: vec![vec![0.5; 4_410]],
        sample_rate: 44_100,
    };
    vibez_audio_io::file_io::write_wav_file(&path, &audio).unwrap();
    path
}

fn browser(message: BrowserMsg) -> Message {
    Message::Browser(message)
}

/// A clip launcher holding one audio track, which is not where drops on
/// empty space land.
pub(super) fn launcher() -> (App, TrackId) {
    let mut app = app();
    app.state.browser.audition_mode = AuditionMode::Raw;
    app.state.view.workspace = crate::state::Workspace::Perform;
    app.state.perform.layout = vibez_project::PerformLayout::Clips;
    let existing = ProjectTrack::new(TrackId::new(), "Audio 1".into(), 0);
    let id = existing.id;
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(existing);
    app.state
        .perform
        .sync_project_tracks(&app.state.project_tracks.tracks);
    (app, id)
}

/// The Sections layout on a selected Section with one audio track.
pub(super) fn sections() -> (App, TrackId, SectionId) {
    let (mut app, existing) = launcher();
    app.state.perform.layout = vibez_project::PerformLayout::Sections;
    let section = crate::domains::perform::Section::new(0);
    let section_id = section.id;
    Arc::make_mut(&mut app.state.perform.sections).insert(section);
    let _ = app.update(Message::Perform(
        crate::domains::perform::PerformMsg::SelectSection(section_id),
    ));
    (app, existing, section_id)
}

pub(super) fn browser_drag(app: &mut App, path: PathBuf) {
    app.state.browser.drag_label = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    app.state.browser.drag_source = Some(MediaSourceRef::LocalFile { path });
}

pub(super) fn the_new_track(app: &App, existing: TrackId) -> &ProjectTrack {
    let added: Vec<_> = app
        .state
        .project_tracks
        .tracks
        .iter()
        .filter(|track| track.id != existing)
        .collect();
    let [track] = added[..] else {
        panic!("one new track, found {}", added.len());
    };
    track
}

pub(super) fn section_clip_starts(app: &App, section_id: SectionId, track_id: TrackId) -> Vec<f64> {
    let samples_per_beat = app.state.transport.samples_per_beat();
    let section = app.state.perform.sections.by_id(section_id).unwrap();
    section
        .timeline
        .get(track_id)
        .map(|content| {
            content
                .note_clips
                .iter()
                .map(|clip| clip.position_beats)
                .chain(
                    content
                        .clips
                        .iter()
                        .map(|clip| clip.span(samples_per_beat).start_beats),
                )
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test]
async fn drums_released_beside_the_last_launcher_column_get_a_drum_rack_track_and_that_slot() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, existing) = launcher();
    browser_drag(&mut app, drum_phrase(directory.path()));
    let _ = app.update(browser(BrowserMsg::DragHoverNewTrackSlot { row: 2 }));

    run_until_idle(&mut app, Message::ImportLauncherClipOnNewTrack { row: 2 }).await;

    let track = the_new_track(&app, existing);
    assert!(track.kind.is_midi());
    assert_eq!(track.instrument_kind, Some(InstrumentKind::DrumRack));
    let clip = app.state.perform.clips.at(track.id, 2).expect("slot clip");
    assert_eq!(clip.name(), "Rebeater P3");
    assert!(app.state.browser.drag_source.is_none());
}

#[tokio::test]
async fn audio_from_another_app_beside_the_last_launcher_column_gets_an_audio_track() {
    let directory = tempfile::tempdir().unwrap();
    let path = kick(directory.path());
    let (mut app, existing) = launcher();
    let _ = app.update(browser(BrowserMsg::DesktopFileHovered(path.clone())));
    let _ = app.update(browser(BrowserMsg::DragHoverNewTrackSlot { row: 1 }));

    run_until_idle(&mut app, browser(BrowserMsg::DesktopFileDropped(path))).await;

    let track = the_new_track(&app, existing);
    assert_eq!(track.kind, TrackKind::Audio);
    assert!(app.state.perform.clips.at(track.id, 1).is_some());
}

#[tokio::test]
async fn the_ghost_waits_in_the_new_columns_slot_under_the_pointer() {
    let directory = tempfile::tempdir().unwrap();
    let path = drum_phrase(directory.path());
    let (mut app, _) = launcher();
    run_until_idle(&mut app, browser(BrowserMsg::DesktopFileHovered(path))).await;

    let _ = app.update(browser(BrowserMsg::DragHoverNewTrackSlot { row: 3 }));

    let ghost = app
        .state
        .browser
        .drop_ghost(app.state.transport.bpm)
        .unwrap();
    assert_eq!(ghost.place, GhostPlace::NewTrackSlot { row: 3 });
    assert_eq!(ghost.media.new_track(), NewTrack::Midi { drum_rack: true });
    assert_eq!(
        app.state.browser.drop_ghost_slot().unwrap().name(),
        "Rebeater P3"
    );
}

#[tokio::test]
async fn drums_released_under_a_sections_tracks_get_a_drum_rack_track_in_that_section() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, existing, section_id) = sections();
    browser_drag(&mut app, drum_phrase(directory.path()));
    let _ = app.update(browser(BrowserMsg::DragHoverEmptyArrangement { beat: 4.0 }));

    run_until_idle(&mut app, Message::DropSampleOnEmptyArrangement).await;

    let track = the_new_track(&app, existing);
    assert_eq!(track.instrument_kind, Some(InstrumentKind::DrumRack));
    assert_eq!(section_clip_starts(&app, section_id, track.id), vec![4.0]);
    assert!(app
        .state
        .arrange_content(track.id)
        .is_none_or(|content| content.note_clips.is_empty()));
}

#[tokio::test]
async fn audio_from_another_app_under_a_sections_tracks_gets_an_audio_track_in_that_section() {
    let directory = tempfile::tempdir().unwrap();
    let path = kick(directory.path());
    let (mut app, existing, section_id) = sections();
    let _ = app.update(browser(BrowserMsg::DesktopFileHovered(path.clone())));
    let _ = app.update(browser(BrowserMsg::DragHoverEmptyArrangement { beat: 2.0 }));

    run_until_idle(&mut app, browser(BrowserMsg::DesktopFileDropped(path))).await;

    let track = the_new_track(&app, existing);
    assert_eq!(track.kind, TrackKind::Audio);
    let starts = section_clip_starts(&app, section_id, track.id);
    assert_eq!(starts.len(), 1);
    assert!((starts[0] - 2.0).abs() < 1e-6);
    assert!(app
        .state
        .arrange_content(track.id)
        .is_none_or(|content| content.clips.is_empty()));
}

#[test]
fn audio_for_a_section_deleted_while_it_was_prepared_adds_no_track() {
    let (mut app, existing, _) = sections();
    let gone = crate::domains::perform::Section::new(1).id;
    let payload = crate::message::PreparedBrowserImport {
        treatment: crate::state::AuditionImportInput {
            mode: AuditionMode::Raw,
            source_bpm: None,
        },
        audio: Arc::new(DecodedAudio {
            channels: vec![vec![0.5; 4_410]],
            sample_rate: 44_100,
        }),
        original_audio: None,
        name: "kick.wav".into(),
        source: crate::state::UiClip::test_source(),
    };

    let _ = app.apply_browser_import_prepared(
        crate::message::BrowserImportTarget::SectionNewTrackAt {
            section_id: gone,
            position_samples: 0,
        },
        payload,
    );

    assert!(app
        .state
        .project_tracks
        .tracks
        .iter()
        .all(|track| track.id == existing));
}
