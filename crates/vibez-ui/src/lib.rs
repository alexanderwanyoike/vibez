mod app;
mod domains;
pub mod icons;
mod message;
pub mod plugin_window;
mod state;
mod theme;
mod ui_settings;
mod warp;
pub mod widgets;

pub fn run() -> iced::Result {
    app::run()
}
