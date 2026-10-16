use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Element, Length};
use vibez_core::{
    id::{EffectId, TrackId},
    routing::{ExternalInputId, SourceTap},
};

use crate::{
    domains::{devices::DevicesMsg, devices::DevicesState},
    message::Message,
    state::{ProjectTrack, UiEffect},
    theme as th,
    widgets::device_controls::dropdown,
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
            SourceTap::BeforeEffects => "Pre FX",
            SourceTap::AfterEffects => "Post FX",
            SourceTap::AfterFader => "Post Fader",
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
    let fill = (proportion * 1000.0).round() as u16;
    let mut bar = row![];
    if fill > 0 {
        bar = bar.push(
            container(Space::new(Length::Fill, Length::Fixed(2.0)))
                .width(Length::FillPortion(fill))
                .style(move |_theme| container::Style {
                    background: Some(color.into()),
                    ..Default::default()
                }),
        );
    }
    if fill < 1000 {
        bar = bar.push(Space::new(
            Length::FillPortion(1000 - fill),
            Length::Fixed(2.0),
        ));
    }
    container(bar.width(Length::Fill))
        .width(Length::Fill)
        .style(|_theme| container::Style {
            background: Some(th::bg_dark().into()),
            ..Default::default()
        })
        .into()
}

/// Fixed width of the sidechain section, shared with the card width math.
pub const WIDTH: f32 = 132.0;

/// Whether the device shows a sidechain column: it has a supported input, or
/// routes that no supported input matches any more.
pub fn shows_for(effect: &UiEffect) -> bool {
    effect.external_inputs.iter().any(|input| input.supported()) || !effect.sidechains.is_empty()
}

pub fn view<'a>(
    receiver: TrackId,
    effect: &'a UiEffect,
    sources: &'a [ProjectTrack],
    buses: &'a [ProjectTrack],
    state: &DevicesState,
) -> Option<Element<'a, Message>> {
    if !shows_for(effect) {
        return None;
    }
    let inputs: Vec<_> = effect
        .external_inputs
        .iter()
        .filter(|input| input.supported())
        .collect();
    let unavailable: Vec<_> = effect
        .sidechains
        .iter()
        .filter(|route| !inputs.iter().any(|input| route.matches(input)))
        .collect();
    let mut content = column![crate::widgets::device_chrome::section_label("Sidechain")].spacing(6);
    for input in &inputs {
        let id = input.id;
        let effect_id = effect.id;
        let route = effect.sidechains.iter().find(|route| route.matches(input));
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
        let mut selected = route.map_or_else(
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
        if route.is_some() && effect.inactive_sidechains.contains(&id) {
            selected.name.push_str(" (inactive)");
        }
        let source = dropdown(choices, selected, move |choice| {
            source_message(receiver, effect_id, id, choice.id)
        });
        let mut controls = column![].spacing(4);
        if inputs.len() > 1 {
            controls = controls.push(text(&input.name).size(9).color(th::text_dim()));
        }
        controls = controls.push(source);
        if let Some(route) = route {
            controls = controls.push(meter(
                state
                    .sidechain_meters
                    .get(&(effect_id, id))
                    .copied()
                    .unwrap_or_default(),
            ));
            let taps = allowed
                .iter()
                .find(|option| option.source == route.source)
                .map(|option| option.taps.as_slice())
                .unwrap_or_default();
            if !taps.is_empty() {
                controls = controls.push(dropdown(
                    taps.iter().copied().map(TapChoice).collect(),
                    TapChoice(route.tap),
                    move |choice| {
                        Message::Devices(DevicesMsg::SetSidechainTap {
                            track_id: receiver,
                            effect_id,
                            input_id: id,
                            tap: choice.0,
                        })
                    },
                ));
            } else {
                controls = controls.push(
                    text(TapChoice(route.tap).to_string())
                        .size(9)
                        .color(th::text_muted()),
                );
            }
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
        .padding([8, 8])
        .width(Length::Fixed(WIDTH))
        .height(Length::Fill)
        .into(),
    )
}
