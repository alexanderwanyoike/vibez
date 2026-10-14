//! Native non-scrolling sidechain selectors over the shared validated routing model.

use crate::{
    domains::{
        compressor::{CompressorMsg, CompressorPanel},
        devices::{DevicesMsg, DevicesState},
        native_sidechain,
    },
    message::Message,
    state::{ProjectTrack, UiEffect},
    theme as th,
    widgets::device_chrome::section_label,
};
use iced::{
    widget::{button, column, pick_list, row, text, tooltip},
    Color, Element, Length,
};
use vibez_core::id::TrackId;

fn selector<'a, T: Clone + ToString + PartialEq + 'a>(
    options: Vec<T>,
    selected: T,
    on_select: impl Fn(T) -> Message + 'a,
    accent: Color,
) -> Element<'a, Message> {
    pick_list(options, Some(selected), on_select)
        .width(Length::Fill)
        .padding([1, 3])
        .text_size(10)
        .style(move |_, status| pick_list::Style {
            text_color: th::text(),
            placeholder_color: th::text_muted(),
            handle_color: th::text_dim(),
            background: th::bg_dark().into(),
            border: iced::Border {
                color: if matches!(
                    status,
                    pick_list::Status::Opened | pick_list::Status::Hovered
                ) {
                    accent
                } else {
                    th::border()
                },
                width: 1.0,
                radius: 2.0.into(),
            },
        })
        .menu_style(move |_| iced::widget::overlay::menu::Style {
            background: th::bg_surface().into(),
            border: iced::Border {
                color: th::border(),
                width: 1.0,
                radius: 2.0.into(),
            },
            text_color: th::text(),
            selected_text_color: accent,
            selected_background: th::bg_hover().into(),
        })
        .into()
}
fn field<'a>(label: &'static str, control: Element<'a, Message>) -> Element<'a, Message> {
    column![section_label(label), control]
        .spacing(3)
        .width(Length::Fill)
        .into()
}

#[allow(clippy::too_many_arguments)]
pub fn view<'a>(
    receiver: TrackId,
    effect: &UiEffect,
    sources: &[ProjectTrack],
    buses: &[ProjectTrack],
    state: &DevicesState,
    panel: &CompressorPanel,
    accent: Color,
    vertical: bool,
) -> Element<'a, Message> {
    let Some(model) =
        native_sidechain::presentation(effect, sources, buses, state, panel.sidechain_input)
    else {
        return text("No detector input")
            .size(9)
            .color(th::text_dim())
            .into();
    };
    let id = effect.id;
    let input_id = model.input.id;
    let mut fields = Vec::new();
    if model.inputs.len() > 1 {
        fields.push(field(
            "Input",
            selector(
                model.inputs,
                model.input,
                move |choice| {
                    Message::CompressorControl(CompressorMsg::SidechainInput(
                        receiver, id, choice.id,
                    ))
                },
                accent,
            ),
        ));
    }
    let source = selector(
        model.sources,
        model.source.clone(),
        move |choice| {
            Message::Devices(DevicesMsg::SetSidechainSource {
                track_id: receiver,
                effect_id: id,
                input_id,
                source: choice.id,
            })
        },
        accent,
    );
    let explanation = format!(
        "{}{}",
        model.source.label,
        model
            .warning
            .map_or(String::new(), |warning| format!(" · {warning}"))
    );
    fields.push(field(
        "Sidechain",
        tooltip(source, text(explanation).size(9), tooltip::Position::Top).into(),
    ));
    if model.assigned {
        let tap = if model.taps.is_empty() {
            text(model.tap.to_string())
                .wrapping(iced::widget::text::Wrapping::None)
                .size(10)
                .color(th::text_dim())
                .into()
        } else {
            selector(
                model.taps,
                model.tap,
                move |choice| {
                    Message::Devices(DevicesMsg::SetSidechainTap {
                        track_id: receiver,
                        effect_id: id,
                        input_id,
                        tap: choice.0,
                    })
                },
                accent,
            )
        };
        fields.push(field("Tap", tap));
        fields.push(
            button(text("Disconnect").size(9))
                .padding([2, 4])
                .on_press(Message::Devices(DevicesMsg::SetSidechainSource {
                    track_id: receiver,
                    effect_id: id,
                    input_id,
                    source: None,
                }))
                .style(move |_, _| button::Style {
                    text_color: accent,
                    background: Some(th::bg_dark().into()),
                    border: iced::Border {
                        color: th::border(),
                        width: 1.0,
                        radius: 2.0.into(),
                    },
                    ..Default::default()
                })
                .into(),
        );
    }
    if vertical {
        let mut controls = column(fields).spacing(5).width(Length::Fill);
        if let Some(warning) = model.warning {
            controls = controls.push(
                text(warning)
                    .size(8)
                    .color(th::text_dim())
                    .wrapping(iced::widget::text::Wrapping::None),
            );
        }
        if model.assigned {
            let db = vibez_core::level::amplitude_to_db(model.raw_peak);
            let value = if db.is_finite() {
                format!("{db:.1}")
            } else {
                "-inf".into()
            };
            controls = controls.push(
                text(format!("Raw external {value} dBFS"))
                    .size(8)
                    .color(th::text_dim()),
            );
        }
        controls.into()
    } else {
        if let Some(warning) = model.warning {
            fields.push(
                text(warning)
                    .size(8)
                    .color(th::text_dim())
                    .wrapping(iced::widget::text::Wrapping::None)
                    .into(),
            );
        }
        row(fields)
            .spacing(6)
            .align_y(iced::Alignment::Center)
            .width(Length::Fill)
            .into()
    }
}
