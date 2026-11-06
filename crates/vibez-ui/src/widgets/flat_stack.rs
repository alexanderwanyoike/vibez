//! Layers over one another that draw in the layer around them.
//!
//! iced's own stack draws each upper layer in a render layer clipped to the
//! viewport it is handed, and a sideways scroll area hands its content its
//! whole height even inside an area scrolled up and down. Render layers do
//! not clip to their parents, so those layers spill out of the outer area.
//! Drawn in the surrounding layer instead, they clip with it.

use iced::advanced::widget::{tree, Operation, Tree};
use iced::advanced::{layout, mouse, overlay, renderer, Clipboard, Layout, Shell, Widget};
use iced::{event, Element, Event, Length, Rectangle, Size, Vector};

pub struct FlatStack<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    width: Length,
    height: Length,
    layers: Vec<Element<'a, Message, Theme, Renderer>>,
}

/// `layers` over one another, the first underneath, sized by the first
/// unless sized otherwise.
pub fn flat_stack<'a, Message, Theme, Renderer>(
    layers: impl IntoIterator<Item = Element<'a, Message, Theme, Renderer>>,
) -> FlatStack<'a, Message, Theme, Renderer> {
    FlatStack {
        width: Length::Shrink,
        height: Length::Shrink,
        layers: layers.into_iter().collect(),
    }
}

impl<Message, Theme, Renderer> FlatStack<'_, Message, Theme, Renderer> {
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for FlatStack<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::stateless()
    }

    fn children(&self) -> Vec<Tree> {
        self.layers.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.layers);
    }

    fn size(&self) -> Size<Length> {
        Size::new(self.width, self.height)
    }

    fn layout(
        &self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let limits = limits.width(self.width).height(self.height);
        let Some((base, rest)) = self.layers.split_first() else {
            return layout::Node::new(limits.resolve(self.width, self.height, Size::ZERO));
        };
        let base = base
            .as_widget()
            .layout(&mut tree.children[0], renderer, &limits);
        let size = limits.resolve(self.width, self.height, base.size());
        let inner = layout::Limits::new(Size::ZERO, size);
        let nodes = std::iter::once(base)
            .chain(
                rest.iter()
                    .zip(&mut tree.children[1..])
                    .map(|(layer, tree)| layer.as_widget().layout(tree, renderer, &inner)),
            )
            .collect();
        layout::Node::with_children(size, nodes)
    }

    fn operate(
        &self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(None, layout.bounds(), &mut |operation| {
            for ((layer, state), layout) in self
                .layers
                .iter()
                .zip(&mut tree.children)
                .zip(layout.children())
            {
                layer
                    .as_widget()
                    .operate(state, layout, renderer, operation);
            }
        });
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
        self.layers
            .iter_mut()
            .rev()
            .zip(tree.children.iter_mut().rev())
            .zip(layout.children().rev())
            .map(|((layer, state), layout)| {
                layer.as_widget_mut().on_event(
                    state,
                    event.clone(),
                    layout,
                    cursor,
                    renderer,
                    clipboard,
                    shell,
                    viewport,
                )
            })
            .find(|status| *status == event::Status::Captured)
            .unwrap_or(event::Status::Ignored)
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.layers
            .iter()
            .rev()
            .zip(tree.children.iter().rev())
            .zip(layout.children().rev())
            .map(|((layer, state), layout)| {
                layer
                    .as_widget()
                    .mouse_interaction(state, layout, cursor, viewport, renderer)
            })
            .find(|interaction| *interaction != mouse::Interaction::None)
            .unwrap_or_default()
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
        for ((layer, state), layout) in self
            .layers
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
        {
            layer
                .as_widget()
                .draw(state, renderer, theme, style, layout, cursor, viewport);
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        overlay::from_children(&mut self.layers, tree, layout, renderer, translation)
    }
}

impl<'a, Message, Theme, Renderer> From<FlatStack<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: 'a + renderer::Renderer,
{
    fn from(stack: FlatStack<'a, Message, Theme, Renderer>) -> Self {
        Element::new(stack)
    }
}
