//! A Clip drag driven through the lane canvas is one undo step, and Escape
//! puts the Clip back in the project and the engine.

use std::collections::HashSet;

use iced::widget::canvas::{self, Program};
use iced::{mouse, Color, Point, Rectangle, Size};

use super::*;
use crate::domains::project::ProjectMsg;
use crate::state::{GridConfig, ProjectTrack, SnapGrid, UiNoteClip};
use crate::timeline_geometry::TimelineGeometry;
use crate::widgets::timeline::{ClipInteractionState, TrackClipCanvas};
use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::{MidiNote, TrackKind};
use vibez_core::perform::GrooveGrid;
use vibez_engine::engine::{AudioEngine, AudioProcessBlock};

const ZOOM: f32 = 1.0;
const START_BEAT: f64 = 4.0;
/// Inside the Clip's title strip, which moves it.
const TITLE_Y: f32 = 6.0;

struct Song {
    app: App,
    engine: AudioEngine,
    track: ProjectTrack,
    lane: ClipInteractionState,
}

/// One MIDI track holding an 8 beat note Clip at beat 4, resident in a
/// real engine.
fn song() -> Song {
    let mut app = super::test_support::app();
    let (engine, commands, events) = AudioEngine::new();
    let mut engine = engine;
    app.cmd_tx =
        crate::domains::EngineCommandQueue::new(commands, engine.take_transport_sender().unwrap());
    app.event_rx = Some(events);
    let mut track = ProjectTrack::new(TrackId::new(), "Keys".into(), 0);
    track.kind = TrackKind::Midi;
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(track.clone());
    app.state
        .arrange_content_mut(track.id)
        .note_clips
        .push(UiNoteClip {
            id: ClipId::new(),
            name: "Pattern".into(),
            position_beats: START_BEAT,
            duration_beats: 8.0,
            notes: vec![MidiNote {
                pitch: 60,
                velocity: 100,
                start_beat: 0.0,
                duration_beats: 8.0,
            }],
            selected_notes: Default::default(),
            start_marker_beats: 0.0,
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 8.0,
            groove_grid: GrooveGrid::Off,
        });
    app.replay_track_to_engine(&track);
    let mut song = Song {
        app,
        engine,
        track,
        lane: ClipInteractionState::default(),
    };
    song.render();
    song
}

impl Song {
    fn render(&mut self) {
        self.engine
            .process_block(AudioProcessBlock::new(&mut [], 2));
        self.app.poll_engine_events();
    }

    /// The lane as the view builds it from the current state.
    fn canvas(&self) -> TrackClipCanvas {
        let content = self.app.state.arrange_content(self.track.id).unwrap();
        TrackClipCanvas::from_track(
            &self.track,
            content,
            0.0,
            ZOOM,
            GridConfig::new(SnapGrid::EIGHTH, true, false, 0),
            0.0,
            1600.0,
            64.0,
            self.app.state.transport.sample_rate,
            true,
            Color::BLACK,
            self.app.state.transport.bpm,
            self.track.id,
            vec![self.track.id],
            HashSet::new(),
            false,
            0.0,
            0.0,
            false,
            0.0,
            0.0,
            None,
            false,
            None,
            None,
        )
    }

    /// Sends `event` at the pointer over `beat` to the lane, and whatever it
    /// emits to the app.
    fn pointer(&mut self, event: mouse::Event, beat: f64) {
        let x = TimelineGeometry::from_zoom(ZOOM, 0.0).beat_to_x(beat);
        let canvas = self.canvas();
        let (_, message) = canvas.update(
            &mut self.lane,
            canvas::Event::Mouse(event),
            Rectangle::new(Point::ORIGIN, Size::new(1600.0, 70.0)),
            mouse::Cursor::Available(Point::new(x, TITLE_Y)),
        );
        if let Some(message) = message {
            let _ = self.app.update(message);
        }
    }

    fn press(&mut self, beat: f64) {
        self.pointer(mouse::Event::ButtonPressed(mouse::Button::Left), beat);
    }

    fn drag_to(&mut self, beat: f64) {
        let x = TimelineGeometry::from_zoom(ZOOM, 0.0).beat_to_x(beat);
        self.pointer(
            mouse::Event::CursorMoved {
                position: Point::new(x, TITLE_Y),
            },
            beat,
        );
    }

    fn release(&mut self, beat: f64) {
        self.pointer(mouse::Event::ButtonReleased(mouse::Button::Left), beat);
    }

    fn project_start(&self) -> f64 {
        self.app
            .state
            .arrange_content(self.track.id)
            .unwrap()
            .note_clips[0]
            .position_beats
    }

    fn engine_start(&self) -> f64 {
        let clips = &self.engine.tracks()[0].playback_source.note_clips;
        assert_eq!(clips.len(), 1);
        clips[0].position_beats
    }
}

#[test]
fn a_clip_drag_is_one_undo_step() {
    let mut song = song();

    song.press(START_BEAT + 1.0);
    for beat in [7.0, 9.0, 13.0] {
        song.drag_to(beat);
    }
    song.release(13.0);
    song.render();
    assert_eq!(song.project_start(), 12.0);
    assert_eq!(song.engine_start(), 12.0);

    let _ = song.app.update(Message::Project(ProjectMsg::Undo));
    song.render();

    assert_eq!(song.project_start(), START_BEAT);
    assert_eq!(song.engine_start(), START_BEAT);
    assert!(!song.app.state.project.history.can_undo());
}

#[test]
fn escape_mid_drag_puts_the_clip_back_and_leaves_no_undo_step() {
    let mut song = song();
    song.press(START_BEAT + 1.0);
    for beat in [7.0, 9.0] {
        song.drag_to(beat);
    }
    song.render();
    assert_eq!(song.engine_start(), 8.0, "the engine follows the drag");

    let _ = song.app.update(Message::EscapePressed);
    song.render();

    assert_eq!(song.project_start(), START_BEAT);
    assert_eq!(song.engine_start(), START_BEAT);
    assert!(song.app.state.arrangement.clip_drag.is_none());
    assert!(!song.app.state.project.history.can_undo());
    assert!(!song.app.state.project.history.can_redo());

    // The pointer is still down: the cancelled drag must not come back.
    song.drag_to(13.0);
    song.release(13.0);
    song.render();
    assert_eq!(song.project_start(), START_BEAT);
    assert_eq!(song.engine_start(), START_BEAT);
    assert!(!song.app.state.project.history.can_undo());
}

#[test]
fn a_drag_shows_its_preview_until_it_drops() {
    let mut song = song();
    song.press(START_BEAT + 1.0);
    song.drag_to(9.0);

    let preview = &song
        .app
        .state
        .arrangement
        .clip_drag
        .as_ref()
        .expect("a drag in flight")
        .preview;
    assert_eq!(preview.readout, "3.1.1");
    assert_eq!(preview.step.guide.map(|guide| guide.beat), Some(8.0));

    song.release(9.0);
    assert!(song.app.state.arrangement.clip_drag.is_none());
}
