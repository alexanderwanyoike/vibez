//! Element wrapper that publishes a message when a pointer press lands outside.

use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{layout, mouse, overlay, renderer, Clipboard, Layout, Shell, Widget};
use iced::{event, Element, Event, Length, Rectangle, Size, Vector};

pub struct OnBlur<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    enabled: bool,
    message: Message,
}

pub fn on_blur<'a, Message, Theme, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    enabled: bool,
    message: Message,
) -> OnBlur<'a, Message, Theme, Renderer> {
    OnBlur {
        content: content.into(),
        enabled,
        message,
    }
}

fn pointer_pressed_outside(
    enabled: bool,
    event: &Event,
    cursor: mouse::Cursor,
    bounds: Rectangle,
) -> bool {
    let pressed = matches!(
        event,
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(iced::touch::Event::FingerPressed { .. })
    );
    enabled && pressed && !cursor.is_over(bounds)
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for OnBlur<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
    Message: Clone,
{
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
        let status = self.content.as_widget_mut().on_event(
            &mut tree.children[0],
            event.clone(),
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        if pointer_pressed_outside(self.enabled, &event, cursor, layout.bounds()) {
            shell.publish(self.message.clone());
        }
        status
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

impl<'a, Message, Theme, Renderer> From<OnBlur<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a + Clone,
    Theme: 'a,
    Renderer: 'a + renderer::Renderer,
{
    fn from(wrapper: OnBlur<'a, Message, Theme, Renderer>) -> Self {
        Element::new(wrapper)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Point;

    #[test]
    fn pending_input_blurs_only_on_an_outside_pointer_press() {
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(80.0, 24.0));
        let press = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let inside = mouse::Cursor::Available(Point::new(20.0, 10.0));
        let outside = mouse::Cursor::Available(Point::new(120.0, 10.0));

        assert!(!pointer_pressed_outside(true, &press, inside, bounds));
        assert!(pointer_pressed_outside(true, &press, outside, bounds));
        assert!(!pointer_pressed_outside(false, &press, outside, bounds));
        assert!(!pointer_pressed_outside(
            true,
            &Event::Mouse(mouse::Event::CursorMoved {
                position: Point::new(120.0, 10.0),
            }),
            outside,
            bounds,
        ));
    }
}
