//! Shared native and plugin effect header, latency display and device body.

use iced::widget::{container, row, Space};
use iced::{Color, Element, Length};

use crate::icons;
use crate::message::Message;
use crate::state::UiEffect;
use crate::theme as th;
use crate::widgets::device_chrome::{shell, Action, Header};
use crate::widgets::effect_knob::{param_column, EffectKnobWidget};
use vibez_core::id::TrackId;
use vibez_plugin_host::gui::PluginGuiKey;

#[path = "sidechain.rs"]
mod sidechain;
pub use sidechain::view as view_sidechain;

pub struct DeviceBody<'a> {
    pub content: Element<'a, Message>,
    pub width: f32,
    pub padding: iced::Padding,
    /// The device's preset controls, built for the width the header spares.
    pub preset: Option<Box<dyn FnOnce(f32) -> Element<'a, Message> + 'a>>,
    /// The device's Advanced switch, carried in the header.
    pub advanced: Option<Action>,
}

/// Width of an effect card, including its sidechain column when present.
/// The chain sums these to give responsive devices their share.
pub fn card_width(effect: &UiEffect, custom_width: Option<f32>, sidechain: bool) -> f32 {
    let body = if let Some(width) = custom_width {
        width
    } else if effect.plugin_name.is_some() {
        crate::widgets::device_chrome::PLUGIN_CARD_W
    } else if !effect.descriptors.is_empty() && !effect.has_plugin_gui {
        (effect.descriptors.len() as f32 * 62.0 + 24.0).max(150.0)
    } else {
        150.0
    };
    body + if sidechain {
        sidechain::WIDTH + 1.0
    } else {
        0.0
    }
}

pub fn view_effect_slot<'a>(
    track_id: TrackId,
    effect: &'a UiEffect,
    track_color: Color,
    custom_body: Option<DeviceBody<'a>>,
    sidechain: Option<Element<'a, Message>>,
    sample_rate: u32,
) -> Element<'a, Message> {
    let is_bypassed = effect.bypass;
    let has_params = !effect.descriptors.is_empty();
    let has_gui = effect.has_plugin_gui;
    let is_plugin = effect.plugin_name.is_some();

    let display_name = effect
        .plugin_name
        .as_deref()
        .unwrap_or_else(|| effect.effect_type.name());

    // A fixed width: a Fill title strip inside a shrink column collapses
    // the card chrome in iced.
    let card_w = card_width(effect, custom_body.as_ref().map(|body| body.width), false);
    let title_w = card_width(
        effect,
        custom_body.as_ref().map(|body| body.width),
        sidechain.is_some(),
    );
    let latency =
        crate::domains::device_latency::latency_label(effect.latency_samples, sample_rate);
    let mut header = Header::new(display_name, track_color, latency.clone()).power(
        !is_bypassed,
        Message::toggle_effect_bypass(track_id, effect.id),
    );
    let (custom_body, advanced) = match custom_body {
        Some(DeviceBody {
            content,
            padding,
            preset,
            advanced,
            ..
        }) => {
            header.preset = preset;
            (Some((content, padding)), advanced)
        }
        None => (None, None),
    };
    if let Some(advanced) = advanced {
        header = header.action(advanced);
    }
    let header = header
        .action(Action::new(
            icons::CHEVRON_UP,
            "Move earlier",
            Message::move_effect_up(track_id, effect.id),
        ))
        .action(Action::new(
            icons::CHEVRON_DOWN,
            "Move later",
            Message::move_effect_down(track_id, effect.id),
        ))
        .action(Action::new(
            icons::X,
            "Remove device",
            Message::remove_effect(track_id, effect.id),
        ));

    // ── Body ─────────────────────────────────────────────────
    let body: Element<'a, Message> = if let Some((content, padding)) = custom_body {
        container(content)
            .padding(padding)
            .height(Length::Fill)
            .clip(true)
            .into()
    } else if has_params && !has_gui {
        let knob_color = if is_bypassed {
            th::text_muted()
        } else {
            track_color
        };
        // One knob row in the shared column format: the devices
        // panel scrolls horizontally and stacked rows clip.
        let mut knob_row = row![].spacing(6);
        for (i, descriptor) in effect.descriptors.iter().enumerate() {
            let value = effect.params.get(i).copied().unwrap_or(descriptor.default);
            let knob = EffectKnobWidget::new(
                track_id,
                effect.id,
                i,
                value,
                descriptor.min,
                descriptor.max,
                descriptor.default,
                knob_color,
            );
            knob_row = knob_row.push(param_column(
                knob,
                descriptor.name.to_string(),
                crate::widgets::effect_knob::format_value(value, descriptor.unit),
            ));
        }

        container(knob_row)
            .padding([8, 10])
            .height(Length::Fill)
            .clip(true)
            .into()
    } else if is_plugin {
        crate::widgets::device_chrome::plugin_face(
            effect
                .plugin_ref
                .as_ref()
                .map(|device| device.format.as_str()),
            latency,
            has_gui.then_some(PluginGuiKey::Effect {
                track_id,
                effect_id: effect.id,
            }),
        )
    } else {
        container(Space::new(Length::Fixed(120.0), Length::Fixed(2.0)))
            .height(Length::Fill)
            .into()
    };

    // Routing reads left to right: what keys the device, then the device.
    let body: Element<'a, Message> = match sidechain {
        Some(sidechain) => row![
            sidechain,
            crate::widgets::device_chrome::divider(),
            container(body).width(Length::Fixed(card_w))
        ]
        .height(Length::Fill)
        .into(),
        None => body,
    };
    shell(header, body, title_w)
}
