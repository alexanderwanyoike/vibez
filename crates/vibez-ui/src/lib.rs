mod app;
pub mod icons;
mod message;
pub mod plugin_window;
mod state;
mod theme;
pub mod widgets;

pub fn run() -> iced::Result {
    app::run()
}
