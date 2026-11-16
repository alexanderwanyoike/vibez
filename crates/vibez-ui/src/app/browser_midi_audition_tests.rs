//! Selecting a MIDI file in the Browser shows its pattern and auditions it, through the router and a real file read.

use std::path::PathBuf;

use vibez_core::id::TrackId;
use vibez_core::midi::{InstrumentKind, MidiNote, TrackKind};
use vibez_core::midi_file::{MidiFile, MidiFileTrack};
use vibez_core::track::MediaSourceRef;
use vibez_engine::commands::{AuditionStart, EngineCommand};

use super::midi_file_drop_tests::drum_phrase;
use super::test_support::{app, run_until_idle};
use super::*;
use crate::domains::midi_file_import::MidiFileImport;
use crate::state::{AuditionMode, ProjectTrack};

/// How the engine was asked to play each MIDI audition.
type Started = (Option<TrackId>, AuditionStart, bool);

struct Rig {
    app: App,
    received: rtrb::Consumer<EngineCommand>,
    track: TrackId,
    directory: tempfile::TempDir,
}

impl Rig {
    /// A MIDI track playing a synth, selected, with the Browser open.
    fn new() -> Self {
        let mut app = app();
        let (commands, received) = rtrb::RingBuffer::new(64);
        app.cmd_tx = crate::domains::EngineCommandQueue::new(
            commands,
            vibez_engine::transport_ring::transport_ring().0,
        );
        let track = TrackId::new();
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(ProjectTrack::new_instrument(
                track,
                "Bass".into(),
                TrackKind::Instrument(InstrumentKind::SubtractiveSynth),
                0,
            ));
        app.state.arrangement.selected_track = Some(track);
        app.state.browser.open = true;
        app.state.browser.keyboard_focus = true;
        Self {
            app,
            received,
            track,
            directory: tempfile::tempdir().unwrap(),
        }
    }

    fn phrase(&self) -> MediaSourceRef {
        MediaSourceRef::LocalFile {
            path: drum_phrase(self.directory.path()),
        }
    }

    async fn send(&mut self, message: Message) {
        run_until_idle(&mut self.app, message).await;
    }

    fn started(&mut self) -> Vec<Started> {
        std::iter::from_fn(|| self.received.pop().ok())
            .filter_map(|command| match command {
                EngineCommand::StartMidiAudition {
                    target,
                    start,
                    looped,
                    ..
                } => Some((target, start, looped)),
                _ => None,
            })
            .collect()
    }

    fn shown_pitches(&self) -> Option<Vec<u8>> {
        let preview = self.app.state.browser.midi_preview.as_ref()?;
        Some(
            preview
                .pattern
                .notes()
                .iter()
                .map(|note| note.pitch)
                .collect(),
        )
    }
}

#[tokio::test]
async fn clicking_a_midi_file_shows_its_pattern_and_auditions_it_on_the_selected_midi_track() {
    let mut rig = Rig::new();
    let phrase = rig.phrase();

    rig.send(Message::ClickLocalBrowserEntry(phrase)).await;

    assert_eq!(rig.shown_pitches(), Some(vec![36, 38]));
    assert_eq!(
        rig.started(),
        [(Some(rig.track), AuditionStart::NextBar, true)]
    );
}

#[tokio::test]
async fn raw_plays_the_file_once_at_once() {
    let mut rig = Rig::new();
    rig.app.state.browser.audition_mode = AuditionMode::Raw;
    let phrase = rig.phrase();

    rig.send(Message::ClickLocalBrowserEntry(phrase)).await;

    assert_eq!(
        rig.started(),
        [(Some(rig.track), AuditionStart::Immediate, false)]
    );
}

#[tokio::test]
async fn without_a_selected_midi_track_the_preview_voice_plays_the_file() {
    let mut rig = Rig::new();
    rig.app.state.arrangement.selected_track = None;
    let phrase = rig.phrase();

    rig.send(Message::ClickLocalBrowserEntry(phrase)).await;

    assert_eq!(rig.started(), [(None, AuditionStart::NextBar, true)]);
}

#[tokio::test]
async fn with_audition_off_a_click_shows_the_pattern_and_play_auditions_it() {
    let mut rig = Rig::new();
    rig.app.state.browser.audition_enabled = false;
    let phrase = rig.phrase();

    rig.send(Message::ClickLocalBrowserEntry(phrase.clone()))
        .await;
    assert_eq!(rig.shown_pitches(), Some(vec![36, 38]));
    assert!(rig.started().is_empty());
    rig.send(Message::PreviewLocalEntry(phrase)).await;

    assert_eq!(rig.started().len(), 1);
}

#[tokio::test]
async fn play_reads_and_auditions_a_file_that_was_never_clicked() {
    let mut rig = Rig::new();
    let phrase = rig.phrase();

    rig.send(Message::PreviewLocalEntry(phrase)).await;

    assert_eq!(rig.shown_pitches(), Some(vec![36, 38]));
    assert_eq!(rig.started().len(), 1);
}

#[tokio::test]
async fn clicking_the_selected_file_again_and_the_replay_key_audition_it_again() {
    let mut rig = Rig::new();
    let phrase = rig.phrase();
    rig.send(Message::ClickLocalBrowserEntry(phrase.clone()))
        .await;

    rig.send(Message::ClickLocalBrowserEntry(phrase)).await;
    rig.send(Message::ReplaySelectedBrowserSample).await;

    assert_eq!(rig.started().len(), 3);
}

#[tokio::test]
async fn switching_raw_and_warp_restarts_the_file_in_the_new_treatment() {
    let mut rig = Rig::new();
    let phrase = rig.phrase();
    rig.send(Message::ClickLocalBrowserEntry(phrase)).await;

    rig.send(Message::SetAuditionMode(AuditionMode::Raw)).await;

    assert_eq!(
        rig.started(),
        [
            (Some(rig.track), AuditionStart::NextBar, true),
            (Some(rig.track), AuditionStart::Immediate, false),
        ]
    );
}

#[tokio::test]
async fn an_unreadable_file_says_why_and_plays_nothing() {
    let mut rig = Rig::new();
    let path = rig.directory.path().join("broken.mid");
    std::fs::write(&path, b"not midi").unwrap();

    rig.send(Message::ClickLocalBrowserEntry(MediaSourceRef::LocalFile {
        path,
    }))
    .await;

    assert!(rig.app.state.browser.waveform_error.is_some());
    assert!(!rig.app.state.browser.audition_loading);
    assert!(rig.started().is_empty());
}

#[test]
fn a_read_that_finishes_after_another_selection_is_ignored() {
    let mut rig = Rig::new();
    let first = MediaSourceRef::LocalFile {
        path: PathBuf::from("/samples/first.mid"),
    };
    let generation = rig.app.state.browser.begin_audition_load(&first);
    rig.app
        .state
        .browser
        .select_source(MediaSourceRef::LocalFile {
            path: PathBuf::from("/samples/second.mid"),
        });
    let _ = rig.app.state.browser.begin_audition_load(&first);

    let _ = rig.app.update(Message::BrowserMidiPreviewReady {
        source: first,
        generation,
        play: true,
        result: Ok(MidiFileImport {
            name: "first".into(),
            file: MidiFile {
                tracks: vec![MidiFileTrack {
                    name: None,
                    notes: vec![MidiNote {
                        pitch: 36,
                        velocity: 100,
                        start_beat: 0.0,
                        duration_beats: 1.0,
                    }],
                    on_drum_channel: false,
                }],
                length_beats: 4.0,
            },
        }),
    });

    assert!(rig.app.state.browser.midi_preview.is_none());
    assert!(rig.started().is_empty());
}

#[tokio::test]
async fn the_playhead_follows_a_playing_midi_audition_and_wraps_its_loop() {
    let mut rig = Rig::new();
    let phrase = rig.phrase();
    rig.send(Message::ClickLocalBrowserEntry(phrase)).await;
    let samples_per_beat = rig.app.state.transport.beats_to_samples(1.0);
    let browser = &mut rig.app.state.browser;
    browser.audition_playing = true;

    browser.audition_position_frames = samples_per_beat;
    let one_beat_in = browser.midi_audition_playhead(samples_per_beat as f64);
    browser.audition_position_frames = samples_per_beat * 5;
    let a_loop_and_a_beat_in = browser.midi_audition_playhead(samples_per_beat as f64);

    assert_eq!(one_beat_in, Some(0.25));
    assert_eq!(a_loop_and_a_beat_in, Some(0.25));
}

#[test]
fn the_engine_gets_a_preview_voice_at_each_sample_rate() {
    let mut rig = Rig::new();

    rig.app.apply_engine_sample_rate(48_000);

    let commands: Vec<_> = std::iter::from_fn(|| rig.received.pop().ok()).collect();
    assert!(matches!(
        commands.as_slice(),
        [
            EngineCommand::SetSampleRate(48_000),
            EngineCommand::SetAuditionVoice(_)
        ]
    ));
}

#[tokio::test]
async fn an_unreadable_file_stops_the_audition_it_replaces() {
    let mut rig = Rig::new();
    let phrase = rig.phrase();
    rig.send(Message::ClickLocalBrowserEntry(phrase)).await;
    let path = rig.directory.path().join("broken.mid");
    std::fs::write(&path, b"not midi").unwrap();
    while rig.received.pop().is_ok() {}

    rig.send(Message::ClickLocalBrowserEntry(MediaSourceRef::LocalFile {
        path,
    }))
    .await;

    let stopped = std::iter::from_fn(|| rig.received.pop().ok())
        .any(|command| matches!(command, EngineCommand::StopAudition));
    assert!(stopped);
    assert!(rig.app.state.status_text.starts_with("Audition error"));
}

#[tokio::test]
async fn the_playhead_shows_only_on_the_strip_of_the_file_playing() {
    let mut rig = Rig::new();
    rig.app.state.browser.audition_enabled = false;
    let playing = rig.phrase();
    rig.send(Message::PreviewLocalEntry(playing)).await;
    let elsewhere = rig.directory.path().join("elsewhere");
    std::fs::create_dir(&elsewhere).unwrap();
    let other = MediaSourceRef::LocalFile {
        path: drum_phrase(&elsewhere),
    };

    rig.send(Message::ClickLocalBrowserEntry(other)).await;

    let browser = &rig.app.state.browser;
    assert!(browser.audition_playing && browser.midi_preview.is_some());
    assert_eq!(browser.midi_audition_playhead(22_050.0), None);
}

#[tokio::test]
async fn while_a_file_plays_the_footer_names_the_track_it_plays_on() {
    let mut rig = Rig::new();
    let phrase = rig.phrase();
    rig.send(Message::ClickLocalBrowserEntry(phrase)).await;
    rig.app.state.arrangement.selected_track = None;

    let shown = rig
        .app
        .state
        .browser
        .midi_audition_player(super::midi_input::configured_target(&rig.app.state));

    assert_eq!(shown, Some(rig.track));
}

#[tokio::test]
async fn reading_the_playing_file_again_keeps_its_playhead() {
    let mut rig = Rig::new();
    rig.app.state.browser.audition_enabled = false;
    let playing = rig.phrase();
    rig.send(Message::PreviewLocalEntry(playing.clone())).await;
    let elsewhere = rig.directory.path().join("elsewhere");
    std::fs::create_dir(&elsewhere).unwrap();
    rig.send(Message::ClickLocalBrowserEntry(MediaSourceRef::LocalFile {
        path: drum_phrase(&elsewhere),
    }))
    .await;

    rig.send(Message::ClickLocalBrowserEntry(playing)).await;

    assert!(rig
        .app
        .state
        .browser
        .midi_audition_playhead(22_050.0)
        .is_some());
}
