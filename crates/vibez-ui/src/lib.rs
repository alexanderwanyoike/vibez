mod app;
pub mod icons;
mod message;
mod state;
mod theme;
pub mod widgets;

pub fn run() -> iced::Result {
    app::run()
}
