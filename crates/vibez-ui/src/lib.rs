mod app;
mod domains;
pub mod icons;
mod message;
pub mod plugin_window;
mod remote_provider;
mod services;
mod spectrum;
mod state;
mod theme;
mod themes;
mod typography;
mod ui_settings;
mod warp;
pub mod widgets;

pub fn run() -> iced::Result {
    app::run()
}
