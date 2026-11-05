//! Escape during a fade drag on the lane canvas puts the fade back and lets
//! go of the pointer.

use std::collections::HashSet;

use iced::keyboard::key::{Code, Named, Physical};
use iced::keyboard::{Event, Key, Location, Modifiers};
use iced::widget::canvas::{self, Program};
use iced::{mouse, Color, Point, Rectangle};

use super::*;
use crate::state::{GridConfig, ProjectTrack, SnapGrid, UiClip};
use crate::timeline_geometry::TimelineGeometry;
use crate::widgets::timeline::{ClipInteractionState, TrackClipCanvas};
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::track::ClipFades;

/// The fade in handle's row, below the clip's title strip.
const FADE_HANDLE_Y: f32 = 24.0;
const BOUNDS: Rectangle = Rectangle {
    x: 0.0,
    y: 0.0,
    width: 1600.0,
    height: 70.0,
};

struct Lane {
    app: App,
    track: ProjectTrack,
    clip_id: ClipId,
    state: ClipInteractionState,
}

/// One audio track holding a selected 2 second clip at the song start
/// with a fade in of 4 410 frames.
fn lane() -> Lane {
    let mut app = super::test_support::app();
    let track = ProjectTrack::new(TrackId::new(), "Loop".into(), 0);
    let clip_id = ClipId::new();
    let frames = 88_200;
    app.state.arrange_content_mut(track.id).clips.push(UiClip {
        id: clip_id,
        name: "Loop".into(),
        audio: Arc::new(DecodedAudio {
            channels: vec![vec![0.25; frames as usize]],
            sample_rate: 44_100,
        }),
        source: None,
        position: 0,
        source_offset: 0,
        start_marker: 0,
        duration: frames,
        loop_enabled: false,
        loop_start: 0,
        loop_end: frames,
        gain_db: Default::default(),
        fades: ClipFades::new(4_410, 0, frames),
        playback_direction: Default::default(),
        transient_markers: Default::default(),
        warp_markers: Default::default(),
        transpose: Default::default(),
        original_bpm: None,
        warped: false,
        warped_to_bpm: None,
        original_audio: None,
    });
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(track.clone());
    Lane {
        app,
        track,
        clip_id,
        state: ClipInteractionState::default(),
    }
}

impl Lane {
    fn canvas(&self) -> TrackClipCanvas {
        let content = self.app.state.arrange_content(self.track.id).unwrap();
        TrackClipCanvas::from_track(
            &self.track,
            content,
            0.0,
            1.0,
            GridConfig::new(SnapGrid::EIGHTH, false, false, 0),
            0.0,
            BOUNDS.width,
            64.0,
            self.app.state.transport.sample_rate,
            true,
            Color::BLACK,
            self.app.state.transport.bpm,
            self.track.id,
            vec![self.track.id],
            HashSet::from([self.clip_id]),
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

    /// Sends `event` with the pointer at `x` to the lane, and whatever it
    /// emits to the app.
    fn send(&mut self, event: canvas::Event, x: f32) {
        let canvas = self.canvas();
        let (_, message) = canvas.update(
            &mut self.state,
            event,
            BOUNDS,
            mouse::Cursor::Available(Point::new(x, FADE_HANDLE_Y)),
        );
        if let Some(message) = message {
            let _ = self.app.update(message);
        }
    }

    fn mouse(&mut self, event: mouse::Event, x: f32) {
        self.send(canvas::Event::Mouse(event), x);
    }

    fn escape(&mut self, x: f32) {
        self.send(
            canvas::Event::Keyboard(Event::KeyPressed {
                key: Key::Named(Named::Escape),
                modified_key: Key::Named(Named::Escape),
                physical_key: Physical::Code(Code::Escape),
                location: Location::Standard,
                modifiers: Modifiers::empty(),
                text: None,
            }),
            x,
        );
        let _ = self.app.update(Message::EscapePressed);
    }

    fn fade_in(&self) -> u64 {
        self.app.state.arrange_content(self.track.id).unwrap().clips[0]
            .fades
            .fade_in_frames()
    }

    /// The pointer x of the fade in handle.
    fn handle_x(&self) -> f32 {
        let samples_per_beat = self.app.state.transport.samples_per_beat();
        TimelineGeometry::from_zoom(1.0, 0.0).beat_to_x(self.fade_in() as f64 / samples_per_beat)
    }
}

#[test]
fn escape_mid_fade_drag_puts_the_fade_back_and_lets_go() {
    let mut lane = lane();
    let handle = lane.handle_x();
    lane.mouse(mouse::Event::ButtonPressed(mouse::Button::Left), handle);
    for x in [handle + 20.0, handle + 40.0] {
        lane.mouse(
            mouse::Event::CursorMoved {
                position: Point::new(x, FADE_HANDLE_Y),
            },
            x,
        );
    }
    assert!(lane.fade_in() > 4_410, "the fade follows the drag");

    lane.escape(handle + 40.0);

    assert_eq!(lane.fade_in(), 4_410);
    assert!(!lane.app.state.project.history.can_undo());
    let away = mouse::Cursor::Available(Point::new(BOUNDS.width - 10.0, FADE_HANDLE_Y));
    assert_eq!(
        lane.canvas().mouse_interaction(&lane.state, BOUNDS, away),
        lane.canvas()
            .mouse_interaction(&ClipInteractionState::default(), BOUNDS, away),
        "the pointer is no longer held by the drag"
    );
    lane.mouse(
        mouse::Event::CursorMoved {
            position: Point::new(handle + 80.0, FADE_HANDLE_Y),
        },
        handle + 80.0,
    );
    assert_eq!(lane.fade_in(), 4_410);
}
