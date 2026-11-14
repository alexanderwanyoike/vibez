//! An element drawn in a render layer of its own, clipped to what is visible.
//!
//! A scroll area draws its content in a render layer of its own, and every
//! later layer paints over earlier ones, so shapes drawn beside a scroll area
//! in the surrounding layer end up under its content. Drawn in a layer
//! opened after the content, they stay on top, and clipping that layer to
//! the visible part keeps them inside any scroll area around them.

use iced::advanced::widget::{tree, Tree};
use iced::advanced::{layout, mouse, renderer, Layout, Widget};
use iced::{Element, Length, Rectangle, Size};

pub struct TopLayer<'a, Message, Theme, Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
}

/// `content`, drawn over whatever was drawn before it.
pub fn top_layer<'a, Message, Theme, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
) -> TopLayer<'a, Message, Theme, Renderer> {
    TopLayer {
        content: content.into(),
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for TopLayer<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::stateless()
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
        let Some(visible) = layout.bounds().intersection(viewport) else {
            return;
        };
        renderer.with_layer(visible, |renderer| {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                layout,
                cursor,
                viewport,
            );
        });
    }
}

impl<'a, Message, Theme, Renderer> From<TopLayer<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: 'a + renderer::Renderer,
{
    fn from(layer: TopLayer<'a, Message, Theme, Renderer>) -> Self {
        Element::new(layer)
    }
}
