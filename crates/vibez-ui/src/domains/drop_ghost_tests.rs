//! The ghost a drag shows of the Clip its release would make.

use std::path::PathBuf;

use super::*;
use crate::domains::browser::{BrowserAction, BrowserMsg};
use crate::warp::LoopGridFit;
use vibez_core::midi::MidiNote;
use vibez_core::midi_file::{MidiFile, MidiFileTrack};

const BPM: f64 = 120.0;

fn path(file: &str) -> PathBuf {
    PathBuf::from("/tmp/Rebeater drags").join(file)
}

fn source(file: &str) -> MediaSourceRef {
    MediaSourceRef::LocalFile { path: path(file) }
}

/// Two drum hits in a file whose End of Track lands a tick past its bar.
fn drum_phrase() -> MidiFileImport {
    let hit = |pitch, start_beat| MidiNote {
        pitch,
        velocity: 100,
        start_beat,
        duration_beats: 0.25,
    };
    MidiFileImport {
        name: "Rebeater P3".into(),
        file: MidiFile {
            tracks: vec![MidiFileTrack {
                name: None,
                notes: vec![hit(36, 0.0), hit(38, 1.0)],
                on_drum_channel: true,
            }],
            length_beats: 4.01,
        },
    }
}

/// `seconds` of a quiet mono tone at 44.1 kHz.
fn tone(seconds: f64, loop_fit: Option<LoopGridFit>) -> GhostMedia {
    let audio = DecodedAudio {
        channels: vec![vec![0.25; (seconds * 44_100.0) as usize]],
        sample_rate: 44_100,
    };
    GhostMedia::Audio {
        clip: Box::new(audio_clip(
            "loop.wav".into(),
            source("loop.wav"),
            Arc::new(audio),
        )),
        loop_fit,
        peaks: Arc::new(Vec::new()),
    }
}

fn read(file: &str, media: GhostMedia) -> BrowserMsg {
    BrowserMsg::DragGhostRead(Ok(Box::new(DragGhostRead {
        source: source(file),
        media,
    })))
}

fn lane(track_id: TrackId, beat: f64, compatible: bool) -> BrowserMsg {
    BrowserMsg::DragHoverTrack {
        track_id,
        beat,
        compatible,
    }
}

/// A desktop drag of `file` whose read has finished as `media`.
fn dragging(file: &str, media: GhostMedia) -> BrowserState {
    let mut browser = BrowserState::default();
    browser.update(BrowserMsg::DesktopFileHovered(path(file)));
    browser.update(read(file, media));
    browser
}

fn place(browser: &BrowserState) -> Option<GhostPlace> {
    browser.drop_ghost(BPM).map(|ghost| ghost.place)
}

#[test]
fn a_desktop_drag_asks_for_its_file_to_be_read_once() {
    let mut browser = BrowserState::default();

    let action = browser.update(BrowserMsg::DesktopFileHovered(path("Rebeater P3.mid")));

    assert_eq!(action.read_drag_ghost, Some(source("Rebeater P3.mid")));
    let action = browser.update(lane(TrackId::new(), 4.0, true));
    assert_eq!(action.read_drag_ghost, None);
}

#[test]
fn a_browser_drag_asks_for_its_file_to_be_read_when_it_starts_moving() {
    let mut browser = BrowserState::default();
    browser.update(BrowserMsg::BeginPendingDrag {
        source: source("kick.wav"),
        label: "kick.wav".into(),
        origin_x: 0.0,
        origin_y: 0.0,
    });

    let still = browser.update(BrowserMsg::PendingDragMoved { x: 1.0, y: 1.0 });
    let moving = browser.update(BrowserMsg::PendingDragMoved { x: 40.0, y: 0.0 });

    assert_eq!(still.read_drag_ghost, None);
    assert_eq!(moving.read_drag_ghost, Some(source("kick.wav")));
}

#[test]
fn a_midi_ghost_waits_on_the_lane_at_the_hovered_beat_for_the_whole_bars_the_drop_makes() {
    let mut browser = dragging("Rebeater P3.mid", GhostMedia::midi(&drum_phrase()));
    let track_id = TrackId::new();

    browser.update(lane(track_id, 8.0, true));

    let ghost = browser.drop_ghost(BPM).unwrap();
    assert_eq!(
        ghost.place,
        GhostPlace::Lane {
            track_id,
            beat: 8.0
        }
    );
    let landed = drum_phrase().plan(Some(track_id), 8.0, MidiFileLayout::OneClip);
    assert_eq!(ghost.length_beats, landed[0].clip.duration_beats);
    assert_eq!(ghost.length_beats, 4.0);
    assert!(browser.lane_drop_ghost(track_id, BPM).is_some());
    assert!(browser.lane_drop_ghost(TrackId::new(), BPM).is_none());
}

#[test]
fn the_ghost_follows_the_pointer_from_beat_to_beat_and_lane_to_lane() {
    let mut browser = dragging("Rebeater P3.mid", GhostMedia::midi(&drum_phrase()));
    let (first, second) = (TrackId::new(), TrackId::new());
    browser.update(lane(first, 8.0, true));

    browser.update(lane(first, 12.0, true));
    assert_eq!(
        place(&browser),
        Some(GhostPlace::Lane {
            track_id: first,
            beat: 12.0
        })
    );
    browser.update(lane(second, 2.0, true));
    assert_eq!(
        place(&browser),
        Some(GhostPlace::Lane {
            track_id: second,
            beat: 2.0
        })
    );
    assert!(browser.lane_drop_ghost(first, BPM).is_none());
}

#[test]
fn a_raw_audio_ghost_lasts_the_files_duration_at_the_project_tempo() {
    let mut browser = dragging("loop.wav", tone(3.0, None));
    browser.audition_mode = AuditionMode::Raw;

    browser.update(lane(TrackId::new(), 4.0, true));

    assert_eq!(browser.drop_ghost(BPM).unwrap().length_beats, 6.0);
    assert_eq!(browser.drop_ghost(90.0).unwrap().length_beats, 4.5);
}

#[test]
fn a_warp_audio_ghost_lasts_the_whole_bars_its_loop_fits() {
    let fit = LoopGridFit {
        bars: 2,
        source_bpm: 160.0,
    };
    let mut browser = dragging("loop.wav", tone(3.0, Some(fit)));
    browser.audition_mode = AuditionMode::Warp;

    browser.update(lane(TrackId::new(), 4.0, true));

    assert_eq!(browser.drop_ghost(BPM).unwrap().length_beats, 8.0);
}

#[test]
fn empty_space_shows_the_ghost_on_the_new_track_the_drop_makes() {
    let mut midi = dragging("Rebeater P3.mid", GhostMedia::midi(&drum_phrase()));
    let mut audio = dragging("loop.wav", tone(1.0, None));

    for browser in [&mut midi, &mut audio] {
        browser.update(BrowserMsg::DragHoverEmptyArrangement { beat: 4.0 });
        assert_eq!(place(browser), Some(GhostPlace::NewTrack { beat: 4.0 }));
    }
    assert_eq!(
        midi.drop_ghost(BPM).unwrap().media.new_track(),
        NewTrack::Midi { drum_rack: true }
    );
    assert_eq!(
        audio.drop_ghost(BPM).unwrap().media.new_track(),
        NewTrack::Audio
    );
}

#[test]
fn new_tracks_say_what_kind_they_will_be() {
    assert_eq!(NewTrack::Audio.label(), "New audio track");
    assert_eq!(
        NewTrack::Midi { drum_rack: false }.label(),
        "New MIDI track"
    );
    assert_eq!(
        NewTrack::Midi { drum_rack: true }.label(),
        "New MIDI track · Drum Rack"
    );
}

#[test]
fn a_lane_that_turns_the_drag_away_shows_no_ghost_and_keeps_its_refusal() {
    let mut browser = dragging("Rebeater P3.mid", GhostMedia::midi(&drum_phrase()));

    let action = browser.update(lane(TrackId::new(), 4.0, false));

    assert!(browser.drop_ghost(BPM).is_none());
    assert_eq!(
        action.status.as_deref(),
        Some("MIDI can't go on an audio lane")
    );
}

#[test]
fn no_ghost_shows_before_the_file_has_been_read() {
    let mut browser = BrowserState::default();
    browser.update(BrowserMsg::DesktopFileHovered(path("Rebeater P3.mid")));

    browser.update(lane(TrackId::new(), 4.0, true));

    assert!(browser.drop_ghost(BPM).is_none());
}

#[test]
fn a_read_that_finishes_after_its_drag_ended_shows_nothing() {
    let mut browser = BrowserState::default();
    browser.update(BrowserMsg::DesktopFileHovered(path("Rebeater P3.mid")));
    browser.update(BrowserMsg::DesktopDragLeft);
    browser.update(BrowserMsg::DesktopFileHovered(path("other.mid")));

    browser.update(read("Rebeater P3.mid", GhostMedia::midi(&drum_phrase())));
    browser.update(lane(TrackId::new(), 4.0, true));

    assert!(browser.drop_ghost(BPM).is_none());
}

#[test]
fn a_failed_read_shows_no_ghost() {
    let mut browser = BrowserState::default();
    browser.update(BrowserMsg::DesktopFileHovered(path("Rebeater P3.mid")));

    browser.update(BrowserMsg::DragGhostRead(Err("unreadable".into())));
    browser.update(lane(TrackId::new(), 4.0, true));

    assert!(browser.drop_ghost(BPM).is_none());
}

#[test]
fn leaving_the_target_takes_the_ghost_away_until_the_next_one() {
    let mut browser = dragging("Rebeater P3.mid", GhostMedia::midi(&drum_phrase()));
    let track_id = TrackId::new();
    browser.update(lane(track_id, 4.0, true));

    browser.update(BrowserMsg::ClearDragTarget);
    assert!(browser.drop_ghost(BPM).is_none());

    browser.update(BrowserMsg::DragHoverEmptyArrangement { beat: 2.0 });
    browser.update(BrowserMsg::LeaveDragTarget(
        BrowserDropTarget::EmptyArrangement { beat: 0.0 },
    ));
    assert!(browser.drop_ghost(BPM).is_none());

    browser.update(lane(track_id, 6.0, true));
    assert_eq!(
        place(&browser),
        Some(GhostPlace::Lane {
            track_id,
            beat: 6.0
        })
    );
}

#[test]
fn cancelling_or_dropping_ends_the_ghost_with_its_drag() {
    let cancels: [fn(&mut BrowserState) -> BrowserAction; 3] = [
        |browser| browser.update(BrowserMsg::DesktopDragLeft),
        |browser| browser.update(BrowserMsg::CancelDrag("Escape".into())),
        |browser| {
            browser.update(BrowserMsg::DesktopFileDropped(path("Rebeater P3.mid")));
            // The router releases on the target, then ends the drag.
            browser.cancel_media_drag();
            BrowserAction::default()
        },
    ];
    for cancel in cancels {
        let mut browser = dragging("Rebeater P3.mid", GhostMedia::midi(&drum_phrase()));
        browser.update(lane(TrackId::new(), 4.0, true));

        cancel(&mut browser);

        assert!(browser.drop_ghost(BPM).is_none());
        assert!(browser.ghost_cache.is_none());
    }
}

#[test]
fn a_file_hovered_again_while_its_drop_waits_is_read_again() {
    let mut browser = BrowserState::default();
    browser.update(BrowserMsg::DesktopFileHovered(path("Rebeater P3.mid")));
    browser.update(BrowserMsg::DesktopFileDropped(path("Rebeater P3.mid")));

    let action = browser.update(BrowserMsg::DesktopFileHovered(path("Rebeater P3.mid")));

    assert_eq!(action.read_drag_ghost, Some(source("Rebeater P3.mid")));
}

#[test]
fn the_other_files_of_a_multi_file_drag_ask_for_no_read() {
    let mut browser = BrowserState::default();
    browser.update(BrowserMsg::DesktopFileHovered(path("kick.wav")));

    let action = browser.update(BrowserMsg::DesktopFileHovered(path("snare.wav")));

    assert_eq!(action.read_drag_ghost, None);
}
