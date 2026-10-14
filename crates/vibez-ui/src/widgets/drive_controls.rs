//! Native Drive sound controls and filter, quality and metering detail pages.

use crate::{
    domains::{device_controls::DeviceControlState, drive::{DriveMsg, DrivePage, DrivePanel}},
    message::Message,
    state::UiEffect,
    theme as th,
    widgets::{device_chrome::section_label, device_controls::{disclosure_with_label, parameter, parameter_with_heading, selector, value_field}},
};
use iced::{widget::{button, column, container, pick_list, row, text, tooltip}, Color, Element, Length};
use vibez_core::id::TrackId;
use vibez_dsp::drive::param;
use vibez_engine::metering::MeterValues;

pub fn disclosure<'a>(track: TrackId, effect: &UiEffect, open: bool, compact: bool) -> Element<'a, Message> {
    disclosure_with_label(track, effect, open, if compact { "Details" } else { "Advanced" })
}

pub fn primary<'a>(track: TrackId, effect: &'a UiEffect, state: &'a DeviceControlState, accent: Color,
    knobs: Option<f32>, rows: usize) -> Element<'a, Message> {
    let control = |index| parameter(track, effect, state, index, accent, knobs);
    if rows == 2 {
        column![
            container(row![control(param::AMOUNT), control(param::TONE)].spacing(8))
                .height(Length::Fill).align_y(iced::Alignment::Center),
            container(row![control(param::OUTPUT), control(param::MIX)].spacing(8))
                .height(Length::Fill).align_y(iced::Alignment::Center),
        ].spacing(8).height(Length::Fill).into()
    } else {
        row![control(param::AMOUNT), control(param::TONE), control(param::OUTPUT), control(param::MIX)]
            .spacing(6).align_y(iced::Alignment::Center).into()
    }
}

pub fn character<'a>(track: TrackId, effect: &UiEffect, accent: Color) -> Element<'a, Message> {
    selector(track, effect, param::CHARACTER, "Character", &["Legacy", "Soft", "Edge", "Hard"], accent)
}

fn quality<'a>(track: TrackId, effect: &UiEffect, accent: Color) -> Element<'a, Message> {
    tooltip(selector(track, effect, param::QUALITY, "Quality (modern)", &["2x", "4x", "8x"], accent),
        text("Quality applies to Soft, Edge and Hard. Legacy keeps its original shaping. Every mode has the same 64-sample delay.").size(9),
        tooltip::Position::Top).into()
}

fn toggle<'a>(track: TrackId, effect: &UiEffect, index: usize, label: Option<&'static str>, accent: Color) -> Element<'a, Message> {
    let active = effect.params.get(index).copied().unwrap_or(effect.descriptors[index].default) >= 0.5;
    button(text(label.unwrap_or(if active { "On" } else { "Off" })).size(9)).padding([2, 4])
        .on_press(Message::set_effect_param(track, effect.id, index, if active { 0.0 } else { 1.0 }))
        .style(move |_, _| button::Style {
            text_color: if active { accent } else { th::text_dim() },
            background: Some(if active { th::bg_hover() } else { th::bg_dark() }.into()),
            border: iced::Border { color: if active { accent } else { th::border() }, width: 1.0, radius: 2.0.into() },
            ..Default::default()
        }).into()
}

fn frequency<'a>(track: TrackId, effect: &'a UiEffect, state: &'a DeviceControlState, accent: Color,
    enabled: usize, index: usize, label: &'static str) -> Element<'a, Message> {
    column![section_label(label), row![toggle(track, effect, enabled, None, accent),
        container(value_field(track, effect, state, index, accent)).width(54.0), text("Hz").size(8).color(th::text_dim())]
        .spacing(4).align_y(iced::Alignment::Center)].spacing(3).width(Length::Fill).into()
}

pub fn db(value: f32) -> String {
    let db = vibez_core::level::amplitude_to_db(value);
    if db.is_finite() { format!("{db:.1}") } else { "-inf".into() }
}

fn reading<'a>(label: &'static str, value: f32) -> Element<'a, Message> {
    column![section_label(label), text(db(value)).size(10).font(crate::typography::PERFORM_TECH)
        .wrapping(iced::widget::text::Wrapping::None)].spacing(3).width(Length::Fill).into()
}

#[allow(clippy::too_many_arguments)]
pub fn advanced<'a>(track: TrackId, effect: &'a UiEffect, state: &'a DeviceControlState, panel: &DrivePanel, accent: Color,
    input: MeterValues, output: MeterValues) -> Element<'a, Message> {
    let id = effect.id;
    let pages = pick_list(DrivePage::ALL, Some(panel.page), move |page| Message::DriveControl(DriveMsg::Page(track, id, page)))
        .text_size(10).padding([1, 3]).width(Length::Fill)
        .style(move |_, _| pick_list::Style {
            text_color: accent, placeholder_color: th::text_dim(), handle_color: th::text_dim(), background: th::bg_dark().into(),
            border: iced::Border { color: th::border(), width: 1.0, radius: 2.0.into() },
        })
        .menu_style(move |_| iced::widget::overlay::menu::Style {
            background: th::bg_surface().into(), text_color: th::text(), selected_text_color: accent, selected_background: th::bg_hover().into(),
            border: iced::Border { color: th::border(), width: 1.0, radius: 2.0.into() },
        });
    let content: Element<'a, Message> = match panel.page {
        DrivePage::Sound => primary(track, effect, state, accent, None, 1),
        DrivePage::Filters => row![toggle(track, effect, param::PRE_HP_ENABLED, Some("HP"), accent),
            parameter_with_heading(track, effect, state, param::PRE_HP_FREQUENCY, accent, None, section_label("Pre HP")),
            toggle(track, effect, param::POST_LP_ENABLED, Some("LP"), accent),
            parameter_with_heading(track, effect, state, param::POST_LP_FREQUENCY, accent, None, section_label("Post LP"))]
            .spacing(6).align_y(iced::Alignment::Center).into(),
        DrivePage::Quality => row![character(track, effect, accent), quality(track, effect, accent)].spacing(8).into(),
        DrivePage::Metering => row![reading("In L", input.peak_l), reading("In R", input.peak_r),
            reading("Out L", output.peak_l), reading("Out R", output.peak_r),
            reading("InL RMS", input.rms_l), reading("InR RMS", input.rms_r),
            reading("OutL RMS", output.rms_l), reading("OutR RMS", output.rms_r)].spacing(4).into(),
    };
    row![column![section_label("Details"), pages].spacing(3).width(88.0), container(content).width(Length::Fill)]
        .spacing(8).align_y(iced::Alignment::Center).width(Length::Fill).into()
}

pub fn aside<'a>(track: TrackId, effect: &'a UiEffect, state: &'a DeviceControlState, accent: Color) -> Element<'a, Message> {
    column![disclosure(track, effect, state.advanced, false),
        column![frequency(track, effect, state, accent, param::PRE_HP_ENABLED, param::PRE_HP_FREQUENCY, "Pre HP"),
        frequency(track, effect, state, accent, param::POST_LP_ENABLED, param::POST_LP_FREQUENCY, "Post LP"),
        quality(track, effect, accent)].spacing(8)]
        .spacing(6).width(Length::Fill).into()
}
