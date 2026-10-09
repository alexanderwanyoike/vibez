use iced::widget::{button, column, container, pick_list, row, scrollable, text, Space};
use iced::{Element, Length, Theme};
use vibez_core::{
    id::{EffectId, TrackId},
    routing::{ExternalInputId, SourceTap},
};

use crate::{
    domains::{devices::DevicesMsg, devices::DevicesState},
    message::Message,
    state::{ProjectTrack, UiEffect},
    theme as th,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceChoice {
    id: Option<TrackId>,
    name: String,
}
impl std::fmt::Display for SourceChoice {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TapChoice(SourceTap);
impl std::fmt::Display for TapChoice {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self.0 {
            SourceTap::BeforeEffects => "Before Effects",
            SourceTap::AfterEffects => "After Effects",
            SourceTap::AfterFader => "After Fader",
        })
    }
}

fn source_message(
    track_id: TrackId,
    effect_id: EffectId,
    input_id: ExternalInputId,
    source: Option<TrackId>,
) -> Message {
    Message::Devices(DevicesMsg::SetSidechainSource {
        track_id,
        effect_id,
        input_id,
        source,
    })
}

fn selector<'a, T: Clone + ToString + PartialEq + 'a>(
    options: Vec<T>,
    selected: T,
    on_select: impl Fn(T) -> Message + 'a,
) -> Element<'a, Message> {
    pick_list(options, Some(selected), on_select)
        .width(Length::Fill)
        .padding([3, 5])
        .text_size(9)
        .style(|_theme: &Theme, status| pick_list::Style {
            text_color: th::text(),
            placeholder_color: th::text_muted(),
            handle_color: th::text_dim(),
            background: th::bg_dark().into(),
            border: iced::Border {
                color: if matches!(
                    status,
                    pick_list::Status::Opened | pick_list::Status::Hovered
                ) {
                    th::accent_dim()
                } else {
                    th::border()
                },
                width: 1.0,
                radius: 3.0.into(),
            },
        })
        .menu_style(|_theme: &Theme| iced::widget::overlay::menu::Style {
            background: th::bg_surface().into(),
            border: iced::Border {
                color: th::border(),
                width: 1.0,
                radius: 3.0.into(),
            },
            text_color: th::text(),
            selected_text_color: th::accent(),
            selected_background: th::bg_hover().into(),
        })
        .into()
}

fn meter<'a>(level: (f32, f32)) -> Element<'a, Message> {
    let peak = level.0.max(level.1).max(0.0);
    let proportion = if peak == 0.0 {
        0.0
    } else {
        (20.0 * peak.log10() + 60.0) / 60.0
    }
    .clamp(0.0, 1.0);
    let color = if peak >= 1.0 {
        th::danger()
    } else {
        th::meter_green()
    };
    let filled = container(Space::new(
        Length::Fixed(3.0),
        Length::Fixed(proportion * 18.0),
    ))
    .style(move |_theme| container::Style {
        background: Some(color.into()),
        ..Default::default()
    });
    container(column![
        Space::new(Length::Fixed(3.0), Length::Fixed((1.0 - proportion) * 18.0)),
        filled
    ])
    .width(Length::Fixed(5.0))
    .padding(1)
    .style(|_theme| container::Style {
        background: Some(th::bg_dark().into()),
        ..Default::default()
    })
    .into()
}

pub fn view<'a>(
    receiver: TrackId,
    effect: &'a UiEffect,
    sources: &'a [ProjectTrack],
    buses: &'a [ProjectTrack],
    state: &DevicesState,
) -> Option<Element<'a, Message>> {
    let inputs: Vec<_> = effect
        .external_inputs
        .iter()
        .filter(|input| input.supported())
        .collect();
    let unavailable: Vec<_> = effect
        .sidechains
        .iter()
        .filter(|route| {
            !inputs.iter().any(|input| {
                input.id == route.input_id
                    && (route.input_name.is_empty() || route.input_name == input.name)
            })
        })
        .collect();
    if inputs.is_empty() && unavailable.is_empty() {
        return None;
    }
    let mut content = column![text("SIDECHAIN").size(8).color(th::text_muted())].spacing(7);
    for input in &inputs {
        let id = input.id;
        let effect_id = effect.id;
        let route = effect.sidechains.iter().find(|route| {
            route.input_id == id && (route.input_name.is_empty() || route.input_name == input.name)
        });
        let mut choices = vec![SourceChoice {
            id: None,
            name: "None".into(),
        }];
        let allowed = state
            .sidechain_choices
            .get(&(effect_id, id))
            .map(Vec::as_slice)
            .unwrap_or_default();
        choices.extend(allowed.iter().filter_map(|option| {
            sources
                .iter()
                .chain(buses)
                .find(|source| source.id == option.source)
                .map(|source| SourceChoice {
                    id: Some(source.id),
                    name: source.name.clone(),
                })
        }));
        let selected = route.map_or_else(
            || choices[0].clone(),
            |route| SourceChoice {
                id: Some(route.source),
                name: sources
                    .iter()
                    .chain(buses)
                    .find(|source| source.id == route.source)
                    .map_or_else(
                        || format!("{} (missing)", route.source_name),
                        |source| source.name.clone(),
                    ),
            },
        );
        let source = selector(choices, selected, move |choice| {
            source_message(receiver, effect_id, id, choice.id)
        });
        let mut controls = column![].spacing(4);
        if inputs.len() > 1 {
            controls = controls.push(text(&input.name).size(9).color(th::text_dim()));
        }
        controls = controls.push(
            row![
                text("Source")
                    .size(9)
                    .color(th::text_dim())
                    .width(Length::Fixed(36.0)),
                source,
                meter(
                    state
                        .sidechain_meters
                        .get(&(effect_id, id))
                        .copied()
                        .unwrap_or_default()
                )
            ]
            .spacing(5)
            .align_y(iced::Alignment::Center),
        );
        if let Some(route) = route {
            let taps = allowed
                .iter()
                .find(|option| option.source == route.source)
                .map(|option| option.taps.as_slice())
                .unwrap_or_default();
            if !taps.is_empty() {
                controls = controls.push(
                    row![
                        text("Tap")
                            .size(9)
                            .color(th::text_dim())
                            .width(Length::Fixed(36.0)),
                        selector(
                            taps.iter().copied().map(TapChoice).collect(),
                            TapChoice(route.tap),
                            move |choice| Message::Devices(DevicesMsg::SetSidechainTap {
                                track_id: receiver,
                                effect_id,
                                input_id: id,
                                tap: choice.0
                            })
                        )
                    ]
                    .spacing(5)
                    .align_y(iced::Alignment::Center),
                );
            } else {
                controls = controls.push(
                    text(TapChoice(route.tap).to_string())
                        .size(9)
                        .color(th::text_muted()),
                );
            }
        } else {
            controls = controls.push(text("After Effects").size(9).color(th::text_muted()));
        }
        content = content.push(controls);
    }
    for route in unavailable {
        let id = route.input_id;
        let effect_id = effect.id;
        let name = if route.input_name.is_empty() {
            format!("Input {}", id.0)
        } else {
            route.input_name.clone()
        };
        let source = sources
            .iter()
            .chain(buses)
            .find(|source| source.id == route.source)
            .map_or_else(
                || format!("{} (missing)", route.source_name),
                |source| source.name.clone(),
            );
        content = content.push(
            column![
                text(format!("{name} (unavailable)"))
                    .size(9)
                    .color(th::text_muted()),
                text(source).size(9).color(th::text_dim()),
                button(text("Disconnect").size(9))
                    .padding([2, 5])
                    .on_press(source_message(receiver, effect_id, id, None))
            ]
            .spacing(3),
        );
    }
    Some(
        container(
            scrollable(content).direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::new().width(3).scroller_width(3),
            )),
        )
        .padding([8, 10])
        .width(Length::Fixed(230.0))
        .height(Length::Fixed(th::DEVICE_BODY_H))
        .into(),
    )
}
