//! The control bar's transport buttons and recessed position display.

use iced::widget::{button, canvas, column, container, row, text, text_input};
use iced::{Alignment, Element, Length, Size};
use vibez_core::time::MusicalTime;

use crate::domains::perform::PerformMsg;
use crate::domains::transport::TransportMsg;

use crate::icons;
use crate::message::Message;
use crate::state::{AppState, Workspace};
use crate::theme as th;
use crate::typography;
use crate::widgets::swing_knob::{parse_swing_percent, SwingKnobWidget};

use super::views_control_bar_style as style;
use super::*;

const TRANSPORT_BUTTON_WIDTH: f32 = 34.0;
const PLAY_BUTTON_WIDTH: f32 = 38.0;
const TRANSPORT_BUTTON_HEIGHT: f32 = 30.0;
const POSITION_WIDTH: f32 = 100.0;
/// Wide enough for "PERFORMANCE TIME", so switching workspace never moves
/// the display's other fields.
const TIME_WIDTH: f32 = 96.0;
const BPM_INPUT_WIDTH: f32 = 40.0;
const SWING_WIDTH: f32 = 86.0;

/// What the position display reads.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct PositionReadout {
    pub position: String,
    pub time: String,
    pub time_label: &'static str,
}

/// Perform runs its own clock from zero, so the display follows that clock
/// there and the Arrange cursor everywhere else.
pub(super) fn position_readout(state: &AppState) -> PositionReadout {
    let (seconds, time_label) = if state.view.workspace == Workspace::Perform {
        let seconds = if state.transport.sample_rate == 0 {
            0.0
        } else {
            state.perform.performance_position_samples as f64
                / f64::from(state.transport.sample_rate)
        };
        (seconds, "PERFORMANCE TIME")
    } else {
        (state.position_seconds(), "TIME")
    };
    PositionReadout {
        position: MusicalTime(seconds * state.transport.bpm / 60.0).bar_beat_sixteenth(),
        time: AppState::format_time(seconds),
        time_label,
    }
}

fn transport_button<'a>(
    icon: char,
    width: f32,
    lit: Option<iced::Color>,
    message: Message,
) -> Element<'a, Message> {
    style::segment_button(
        icons::icon(icon).size(14),
        Size::new(width, TRANSPORT_BUTTON_HEIGHT),
        lit,
        message,
    )
}

/// One field of the display: a value over its label.
fn display_field<'a>(
    value: impl Into<Element<'a, Message>>,
    label: &'a str,
    width: f32,
) -> Element<'a, Message> {
    container(
        column![value.into(), style::display_label(label, th::text_muted())]
            .spacing(1)
            .align_x(Alignment::Center),
    )
    .center_x(Length::Fixed(width))
    .center_y(Length::Fill)
    .into()
}

impl App {
    pub(super) fn view_transport(&self) -> Element<'_, Message> {
        row![self.view_transport_buttons(), self.view_position_display()]
            .spacing(10)
            .align_y(Alignment::Center)
            .into()
    }

    fn view_transport_buttons(&self) -> Element<'_, Message> {
        let playing = self.state.transport.is_playing();
        let buttons = row![
            transport_button(
                icons::SKIP_BACK,
                TRANSPORT_BUTTON_WIDTH,
                None,
                Message::Transport(TransportMsg::SeekToBeat(0.0)),
            ),
            transport_button(
                icons::STOP,
                TRANSPORT_BUTTON_WIDTH,
                None,
                Message::Transport(TransportMsg::Stop),
            ),
            transport_button(
                icons::PLAY,
                PLAY_BUTTON_WIDTH,
                playing.then(th::success),
                Message::Transport(if playing {
                    TransportMsg::Stop
                } else {
                    TransportMsg::Play
                }),
            ),
            transport_button(
                icons::CIRCLE,
                TRANSPORT_BUTTON_WIDTH,
                self.state.audio_recording.is_recording().then(th::danger),
                Message::ToggleAudioRecording,
            ),
            transport_button(
                icons::REPEAT,
                TRANSPORT_BUTTON_WIDTH,
                self.state.transport.loop_enabled.then(th::accent),
                Message::Transport(TransportMsg::ToggleArrangementLoop),
            ),
        ]
        .spacing(2);
        container(buttons)
            .padding(style::GROUP_INSET)
            .style(style::group)
            .into()
    }

    fn view_position_display(&self) -> Element<'_, Message> {
        let readout = position_readout(&self.state);
        let position = text(readout.position)
            .font(typography::PERFORM_TECH_STRONG)
            .size(19)
            .color(th::text());
        let time = text(readout.time)
            .font(typography::PERFORM_TECH)
            .size(14)
            .color(th::text());

        let fields = row![
            display_field(position, "BAR  BEAT  16TH", POSITION_WIDTH),
            style::display_divider(),
            display_field(time, readout.time_label, TIME_WIDTH),
            style::display_divider(),
            self.view_bpm_field(),
            style::display_divider(),
            self.view_swing_field(),
        ]
        .height(Length::Fill)
        .align_y(Alignment::Center);

        container(fields)
            .height(Length::Fixed(style::DISPLAY_HEIGHT))
            .padding([0, 2])
            .style(style::display)
            .into()
    }

    fn view_bpm_field(&self) -> Element<'_, Message> {
        let bpm_input = text_input("BPM", &self.state.transport.bpm_text)
            .on_input(|t| Message::Transport(TransportMsg::BpmChanged(t)))
            .on_submit(Message::Transport(TransportMsg::BpmSubmit))
            .font(typography::PERFORM_TECH)
            .size(14)
            .padding([0, 2])
            .width(Length::Fixed(BPM_INPUT_WIDTH))
            .style(style::display_input);
        let nudge = |icon: char, delta: f64| {
            button(icons::icon(icon).size(8))
                .on_press(Message::Transport(TransportMsg::NudgeBpm(delta)))
                .padding([0, 3])
                .style(style::segment(None))
        };
        let nudges = column![
            nudge(icons::CHEVRON_UP, 1.0),
            nudge(icons::CHEVRON_DOWN, -1.0)
        ]
        .spacing(1);

        container(
            row![
                column![bpm_input, style::display_label("BPM", th::text_muted())]
                    .spacing(1)
                    .align_x(Alignment::Center),
                nudges,
            ]
            .spacing(2)
            .align_y(Alignment::Center),
        )
        .padding([0, 6])
        .center_y(Length::Fill)
        .into()
    }

    fn view_swing_field(&self) -> Element<'_, Message> {
        let project_swing = self.state.perform.project_swing();
        let locked = self.state.perform.capture.is_active();
        let submit = parse_swing_percent(self.state.perform.project_swing_input())
            .map(|swing| Message::Perform(PerformMsg::SetProjectSwing(swing.get())))
            .unwrap_or_else(|| {
                Message::Perform(PerformMsg::ProjectSwingInput(format!(
                    "{:.0}",
                    project_swing.get() * 100.0
                )))
            });
        let input = text_input("%", self.state.perform.project_swing_input())
            .font(typography::PERFORM_TECH)
            .size(14)
            .padding([0, 2])
            .width(Length::Fixed(34.0))
            .style(style::display_input);
        let input = if locked {
            input
        } else {
            input
                .on_input(|value| Message::Perform(PerformMsg::ProjectSwingInput(value)))
                .on_submit(submit)
        };
        let knob: Element<'_, Message> =
            canvas(SwingKnobWidget::project_locked(project_swing, locked))
                .width(Length::Fixed(24.0))
                .height(Length::Fixed(24.0))
                .into();
        let label = if locked {
            style::display_label("CAPTURE LOCK", th::accent())
        } else {
            style::display_label("SWING", th::text_muted())
        };

        container(
            column![
                row![knob, input].spacing(4).align_y(Alignment::Center),
                label
            ]
            .align_x(Alignment::Center),
        )
        .center_x(Length::Fixed(SWING_WIDTH))
        .center_y(Length::Fill)
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::{position_readout, PositionReadout};
    use crate::state::{AppState, Workspace};

    #[test]
    fn perform_reads_its_own_zero_based_clock_in_the_display() {
        let mut state = AppState::default();
        state.view.workspace = Workspace::Perform;
        state.transport.sample_rate = 48_000;
        state.transport.bpm = 120.0;
        state.transport.position_samples = 480_000;
        state.perform.performance_position_samples = 156_000;

        assert_eq!(
            position_readout(&state),
            PositionReadout {
                position: "2.3.3".into(),
                time: "00:03.25".into(),
                time_label: "PERFORMANCE TIME",
            }
        );
    }

    #[test]
    fn arrange_reads_its_cursor_as_bar_beat_sixteenth_and_time() {
        let mut state = AppState::default();
        state.view.workspace = Workspace::Arrange;
        state.transport.sample_rate = 48_000;
        state.transport.bpm = 120.0;
        state.transport.position_samples = 156_000;
        state.perform.performance_position_samples = 480_000;

        assert_eq!(
            position_readout(&state),
            PositionReadout {
                position: "2.3.3".into(),
                time: "00:03.25".into(),
                time_label: "TIME",
            }
        );
    }
}
