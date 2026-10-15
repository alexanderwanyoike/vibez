//! Builds Delay's musical choices and native value fields inside measured control cells.

use iced::{widget::{button, column, container, pick_list, row, text, tooltip}, Color, Element, Length};
use vibez_core::{delay_state::DelayState, id::TrackId};
use vibez_dsp::delay::param;
use crate::{domains::{delay_layout::{Control, Page}, delay_values,
    device_controls::DeviceControlState, devices::DevicesMsg}, message::Message,
    state::UiEffect, theme as th, widgets::{device_chrome::section_label, device_controls}};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BeatChoice { division: usize, modifier: usize }
impl std::fmt::Display for BeatChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "1/{}{}", [64, 32, 16, 8, 4, 2, 1][self.division],
            ["", " dotted", " triplet"][self.modifier])
    }
}

fn picker<'a, T: Clone + PartialEq + ToString + 'a>(options: Vec<T>, selected: Option<T>,
    on_select: impl Fn(T) -> Message + 'a, accent: Color) -> Element<'a, Message> {
    pick_list(options, selected, on_select).text_size(10).padding([1, 3]).width(Length::Fill)
        .style(move |_, _| pick_list::Style {
            text_color: accent, placeholder_color: th::text_dim(), handle_color: th::text_dim(),
            background: th::bg_dark().into(), border: iced::Border {
                color: th::border(), width: 1.0, radius: 2.0.into() },
        })
        .menu_style(move |_| iced::widget::overlay::menu::Style {
            background: th::bg_surface().into(), text_color: th::text(), selected_text_color: accent,
            selected_background: th::bg_hover().into(), border: iced::Border {
                color: th::border(), width: 1.0, radius: 2.0.into() },
        }).into()
}

fn musical_time<'a>(track: TrackId, effect: &UiEffect, accent: Color, snapshot: Option<&DelayState>) -> Element<'a, Message> {
    let choices: Vec<_> = (0..7).flat_map(|division|
        (0..3).map(move |modifier| BeatChoice { division, modifier })).collect();
    let selected = choices.iter().find(|choice|
        choice.division as f32 == effect.params.get(param::LEFT_DIVISION).copied().unwrap_or(4.0)
        && choice.modifier as f32 == effect.params.get(param::LEFT_MODIFIER).copied().unwrap_or(0.0)).copied();
    let id = effect.id;
    let control = picker(choices, selected, move |choice| Message::Devices(
        DevicesMsg::SetEffectParams(track, id, vec![(param::LEFT_DIVISION, choice.division as f32),
            (param::LEFT_MODIFIER, choice.modifier as f32)])), accent);
    let title = snapshot.map_or_else(|| "Time: N/A".into(), |state| {
        if state.lines[0].moving { "Time: moving".into() }
        else { format!("Time {:.1} ms", state.lines[0].old_samples * 1000.0 / f64::from(state.sample_rate)) }
    });
    column![section_label(title), control].spacing(3).into()
}

fn binary_choice<'a>(track: TrackId, effect: &UiEffect, index: usize,
    label: &'static str, options: &'static [&'static str], accent: Color,
    width: f32) -> Element<'a, Message> {
    let value = effect.params.get(index).copied().unwrap_or(effect.descriptors[index].default);
    let selected = usize::from(value == 1.0);
    let id = effect.id;
    let segment = |choice: usize, active: bool| {
        button(text(options[choice]).size(10).wrapping(iced::widget::text::Wrapping::None))
            .on_press(Message::set_effect_param(track, id, index, choice as f32))
            .padding([1, 3]).width(Length::Fill).style(move |_, status| button::Style {
                background: Some(if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                    th::bg_hover() } else { th::bg_dark() }.into()),
                text_color: if active { accent } else { th::text() },
                border: iced::Border { color: if active { accent } else { th::border() },
                    width: 1.0, radius: 2.0.into() }, ..Default::default()
            })
    };
    // Text estimates include each button's padding so short pages keep a readable single toggle.
    let longest = options.iter().map(|label| label.chars().count()).max().unwrap_or(0) as f32;
    let content: Element<'a, Message> = if (width - 2.0) * 0.5 >= longest * 6.0 + 6.0 {
        row![segment(0, selected == 0), segment(1, selected == 1)].spacing(2).into()
    } else {
        segment(selected, true).on_press(Message::set_effect_param(
            track, id, index, (1 - selected) as f32)).into()
    };
    column![section_label(label), tooltip(content,
        text(format!("{label}: {} or {}.", options[0], options[1])).size(9),
        tooltip::Position::Top)].spacing(3).into()
}

fn cell<'a>(track: TrackId, effect: &'a UiEffect, state: &'a DeviceControlState,
    accent: Color, control: &Control, snapshot: Option<&DelayState>) -> Element<'a, Message> {
    let index = control.parameters[0];
    if control.parameters.len() > 1 && effect.params.get(param::SYNC) == Some(&1.0) {
        return musical_time(track, effect, accent, snapshot);
    }
    let choice = match index {
        param::SYNC => Some(("Sync", &["Free", "Sync"][..])),
        param::ROUTING => Some(("Mode", &["Stereo", "Ping-pong"][..])),
        param::LINK => Some(("L/R Link", &["Unlinked", "Linked"][..])),
        param::LEFT_DIVISION | param::RIGHT_DIVISION => Some(("Division",
            &["1/64", "1/32", "1/16", "1/8", "1/4", "1/2", "1/1"][..])),
        param::LEFT_MODIFIER | param::RIGHT_MODIFIER => Some(("Modifier",
            &["Straight", "Dotted", "Triplet"][..])),
        param::FEEDBACK_HP_ENABLED | param::FEEDBACK_LP_ENABLED => Some(("Enabled", &["Off", "On"][..])),
        param::TIME_CHANGE => Some(("Time change", &["Crossfade", "Pitch glide"][..])),
        _ => None,
    };
    if let Some((label, options)) = choice {
        if options.len() == 2 {
            binary_choice(track, effect, index, label, options, accent, control.bounds.width)
        } else {
            device_controls::selector(track, effect, index, label, options, accent)
        }
    } else {
        let knob = control.knob_size.filter(|_| matches!(index, param::FEEDBACK | param::MIX | param::DUCK));
        device_controls::parameter_with_policy(track, effect, state, index, accent, knob,
            delay_values::policy(index))
    }
}

pub fn bank<'a>(track: TrackId, effect: &'a UiEffect, state: &'a DeviceControlState,
    accent: Color, controls: &[Control], snapshot: Option<&DelayState>) -> Element<'a, Message> {
    let mut bank = row![].spacing(6);
    for control in controls {
        bank = bank.push(container(cell(track, effect, state, accent, control, snapshot))
            .width(control.bounds.width).height(control.bounds.height));
    }
    let detail = snapshot.map_or_else(|| "Actual timing and Duck attenuation unavailable".into(), |state|
        format!("Applied BPM {:.2}. L old/destination {:.2}/{:.2} ms; R {:.2}/{:.2} ms. Duck {:.1} dB. Right timing is retained while linked.",
            state.bpm, state.lines[0].old_samples * 1000.0 / f64::from(state.sample_rate),
            state.lines[0].destination_samples * 1000.0 / f64::from(state.sample_rate),
            state.lines[1].old_samples * 1000.0 / f64::from(state.sample_rate),
            state.lines[1].destination_samples * 1000.0 / f64::from(state.sample_rate), state.duck_attenuation_db));
    tooltip(bank, text(detail).size(9).width(320.0), tooltip::Position::Top).into()
}

pub fn page_selector<'a>(selected: Page, on_page: impl Fn(Page) -> Message + 'a,
    accent: Color) -> Element<'a, Message> {
    column![section_label("Page"), picker(Page::ALL.to_vec(), Some(selected), on_page, accent)]
        .spacing(3).into()
}
