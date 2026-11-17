//! The app's real view, laid out headlessly at its window size, so tests
//! can point at a place in it as a drag would.

use iced::advanced::widget::operation::{self, Operation, Outcome};
use iced::advanced::widget::Id;
use iced::{mouse, Point, Rectangle, Size};
use iced_runtime::user_interface::{Cache, UserInterface};

use super::drop_target_probe::DropTargetProbe;
use super::App;
use crate::domains::desktop_pointer::PointerTarget;
use crate::message::Message;

/// A software renderer, which lays text out without a window or GPU.
fn renderer() -> iced::Renderer {
    iced::Renderer::Secondary(iced_tiny_skia::Renderer::new(
        iced::Font::DEFAULT,
        iced::Pixels(16.0),
    ))
}

fn with_view<T>(
    app: &App,
    run: impl FnOnce(
        &mut UserInterface<'_, Message, iced::Theme, iced::Renderer>,
        &mut iced::Renderer,
    ) -> T,
) -> T {
    let mut renderer = renderer();
    let size = Size::new(app.state.view.window_width, app.state.view.window_height);
    let mut ui = UserInterface::build(app.view(), size, Cache::default(), &mut renderer);
    run(&mut ui, &mut renderer)
}

/// What the view sends when `events` reach it with the pointer at `point`.
fn messages_at(app: &App, point: Point, events: &[iced::Event]) -> Vec<Message> {
    with_view(app, |ui, renderer| {
        let mut messages = Vec::new();
        let _ = ui.update(
            events,
            mouse::Cursor::Available(point),
            renderer,
            &mut iced::advanced::clipboard::Null,
            &mut messages,
        );
        messages
    })
}

/// What the view sends when the pointer moves to `point`.
pub(super) fn hover_at(app: &App, point: Point) -> Vec<Message> {
    messages_at(
        app,
        point,
        &[iced::Event::Mouse(mouse::Event::CursorMoved {
            position: point,
        })],
    )
}

/// What the view sends when the pointer moves to `point` and the button
/// is released there, as a Browser drag ends.
pub(super) fn release_at(app: &App, point: Point) -> Vec<Message> {
    messages_at(
        app,
        point,
        &[
            iced::Event::Mouse(mouse::Event::CursorMoved { position: point }),
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ],
    )
}

/// The tagged drop target a polled pointer at `point` finds, and the left
/// edge it reports.
pub(super) fn pointer_target_at(app: &App, point: Point) -> Option<(PointerTarget, f32)> {
    with_view(app, |ui, renderer| {
        let mut probe = DropTargetProbe::new(point, app.pointer_targets());
        ui.operate(renderer, &mut operation::black_box(&mut probe));
        match probe.finish() {
            Outcome::Some(found) => found.map(|(target, bounds)| (target, bounds.x)),
            _ => None,
        }
    })
}

/// Where the view laid out the container tagged `target`, before any
/// scrolling.
pub(super) fn bounds_of(app: &App, target: PointerTarget) -> Option<Rectangle> {
    struct Find {
        id: Id,
        found: Option<Rectangle>,
    }
    impl Operation for Find {
        fn container(
            &mut self,
            id: Option<&Id>,
            bounds: Rectangle,
            operate_on_children: &mut dyn FnMut(&mut dyn Operation),
        ) {
            if self.found.is_none() && id == Some(&self.id) {
                self.found = Some(bounds);
            }
            operate_on_children(self);
        }
    }
    let mut find = Find {
        id: Id::from(super::desktop_pointer::target_id(target)),
        found: None,
    };
    with_view(app, |ui, renderer| ui.operate(renderer, &mut find));
    find.found
}
