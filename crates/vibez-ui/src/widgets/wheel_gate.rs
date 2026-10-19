//! Element wrapper that holds the mouse wheel back from its content.
//!
//! iced turns the wheel into sideways movement inside a sideways scroll area,
//! and hands a wheel the area cannot use on to the page around it. Nested in
//! a page that scrolls up and down, both steal from each other; gated, the
//! page keeps the plain wheel and the sideways area all of Shift and the wheel.

use iced::advanced::widget::{tree, Operation, Tree};
use iced::advanced::{layout, mouse, overlay, renderer, Clipboard, Layout, Shell, Widget};
use iced::{event, Element, Event, Length, Rectangle, Size, Vector};

pub struct WheelGate<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
}

/// Shift as the gate last saw it. Tracked from the event stream itself, not
/// the app's view, so a Shift press and a wheel turn delivered together
/// still arrive in order.
#[derive(Default)]
struct GateState {
    shift: bool,
}

/// Wraps `content`, giving it the wheel only while Shift is held.
pub fn shift_wheel<'a, Message, Theme, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
) -> WheelGate<'a, Message, Theme, Renderer> {
    WheelGate {
        content: content.into(),
    }
}

/// What the gate does with a wheel turn over it.
#[derive(Debug, PartialEq)]
enum Wheel {
    /// Leave it to the page: the plain wheel scrolls up and down.
    Pass,
    /// Give it to the content and keep it from the page, even at the end of
    /// the content's travel.
    Keep,
}

fn wheel(shift: bool, event: &Event) -> Option<Wheel> {
    matches!(event, Event::Mouse(mouse::Event::WheelScrolled { .. })).then_some(if shift {
        Wheel::Keep
    } else {
        Wheel::Pass
    })
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for WheelGate<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<GateState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(GateState::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn on_event(
        &mut self,
        tree: &mut Tree,
        event: Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) -> event::Status {
        let gate = tree.state.downcast_mut::<GateState>();
        if let Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers)) = &event {
            gate.shift = modifiers.shift();
        }
        let turn = wheel(gate.shift, &event);
        if turn == Some(Wheel::Pass) {
            return event::Status::Ignored;
        }
        let status = self.content.as_widget_mut().on_event(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        if turn == Some(Wheel::Keep) && cursor.is_over(layout.bounds()) {
            event::Status::Captured
        } else {
            status
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(&mut tree.children[0], layout, renderer, translation)
    }
}

impl<'a, Message, Theme, Renderer> From<WheelGate<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: 'a + renderer::Renderer,
{
    fn from(gate: WheelGate<'a, Message, Theme, Renderer>) -> Self {
        Element::new(gate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plain_wheel_passes_and_shift_with_the_wheel_is_kept() {
        let turn = Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
        });
        let click = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        assert_eq!(wheel(false, &turn), Some(Wheel::Pass));
        assert_eq!(wheel(true, &turn), Some(Wheel::Keep));
        assert_eq!(wheel(false, &click), None);
    }
}
