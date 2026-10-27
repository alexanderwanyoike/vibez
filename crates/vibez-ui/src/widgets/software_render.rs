//! Test-only rendering of a canvas program through iced's tiny-skia
//! software renderer, so drawing can be checked pixel by pixel without a
//! window or GPU.

use iced::advanced::graphics::geometry::Renderer as _;
use iced::advanced::graphics::Viewport;
use iced::advanced::Renderer as _;
use iced::widget::canvas;
use iced::{mouse, Color, Font, Pixels, Rectangle, Size, Vector};

use crate::message::Message;

/// Window colour behind the canvas; nothing in vibez draws pure magenta.
pub const BACKGROUND: [u8; 3] = [255, 0, 255];

/// A rendered window holding one canvas at `bounds`.
pub struct Rendered {
    pixmap: tiny_skia::Pixmap,
    bounds: Rectangle,
}

impl Rendered {
    /// RGB at canvas-local `(x, y)`.
    pub fn rgb(&self, x: f32, y: f32) -> [u8; 3] {
        let px = self
            .pixmap
            .pixel((self.bounds.x + x) as u32, (self.bounds.y + y) as u32)
            .expect("pixel inside the window");
        // iced's tiny-skia backend stores BGRA for the window surface.
        [px.blue(), px.green(), px.red()]
    }

    /// Every pixel of the canvas area, row by row.
    pub fn canvas_pixels(&self) -> Vec<[u8; 3]> {
        let (w, h) = (self.bounds.width as u32, self.bounds.height as u32);
        (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| self.rgb(x as f32, y as f32))
            .collect()
    }
}

/// Draws `program` the way iced's canvas widget does (frame geometry
/// translated to the widget's position) into a window just large enough
/// to hold `bounds`.
pub fn render<P: canvas::Program<Message>>(
    program: &P,
    state: &P::State,
    bounds: Rectangle,
) -> Rendered {
    let mut renderer =
        iced::Renderer::Secondary(iced_tiny_skia::Renderer::new(Font::default(), Pixels(12.0)));
    let geometry = program.draw(
        state,
        &renderer,
        &iced::Theme::Dark,
        bounds,
        mouse::Cursor::Unavailable,
    );
    renderer.with_translation(Vector::new(bounds.x, bounds.y), |renderer| {
        for layer in geometry {
            renderer.draw_geometry(layer);
        }
    });
    let iced::Renderer::Secondary(mut software) = renderer else {
        unreachable!("built as the software renderer")
    };

    let window = Size::new(
        (bounds.x + bounds.width).ceil() as u32,
        (bounds.y + bounds.height).ceil() as u32,
    );
    let mut pixmap = tiny_skia::Pixmap::new(window.width, window.height).expect("window size");
    let mut clip_mask = tiny_skia::Mask::new(window.width, window.height).expect("window size");
    let full = Rectangle::with_size(Size::new(window.width as f32, window.height as f32));
    software.draw::<&str>(
        &mut pixmap.as_mut(),
        &mut clip_mask,
        &Viewport::with_physical_size(window, 1.0),
        &[full],
        Color::from_rgb8(BACKGROUND[0], BACKGROUND[1], BACKGROUND[2]),
        &[],
    );
    Rendered { pixmap, bounds }
}
