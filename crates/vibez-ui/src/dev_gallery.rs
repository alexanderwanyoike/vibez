//! Renders device cards with fixed data so their layout can be checked by
//! screenshot without driving the full application.

use iced::widget::{column, container, row, text};
use iced::{Element, Length, Theme};
use vibez_core::effect::{EffectType, PluginDeviceInfo};
use vibez_core::id::{EffectId, TrackId};
use vibez_core::routing::{
    ExternalInputDescriptor, ExternalInputId, SidechainAssignment, SourceTap,
};

use crate::domains::devices::DevicesState;
use crate::message::Message;
use crate::state::{ProjectTrack, UiEffect};
use crate::theme as th;
use crate::widgets::effect_slot::{view_effect_slot, view_sidechain};

struct Gallery {
    receiver: TrackId,
    sources: Vec<ProjectTrack>,
    effects: Vec<UiEffect>,
    devices: DevicesState,
}

fn plugin(name: &str, format: &str) -> PluginDeviceInfo {
    PluginDeviceInfo {
        format: format.into(),
        uid: name.into(),
        path: std::path::PathBuf::new(),
        name: name.into(),
        state_b64: None,
    }
}

fn effect(
    effect_type: EffectType,
    plugin_ref: Option<PluginDeviceInfo>,
    has_plugin_gui: bool,
    sidechain_input: bool,
    sidechain_source: Option<&ProjectTrack>,
    latency_samples: u32,
) -> UiEffect {
    let builtin = vibez_dsp::factory::create_effect(effect_type, 48_000.0);
    let external_inputs = if plugin_ref.is_some() {
        sidechain_input
            .then(|| ExternalInputDescriptor {
                id: ExternalInputId(1),
                name: "Sidechain".into(),
                channels: 2,
            })
            .into_iter()
            .collect()
    } else {
        builtin.external_inputs().to_vec()
    };
    let sidechains = sidechain_source
        .zip(external_inputs.first())
        .map(|(source, input)| SidechainAssignment {
            input_id: input.id,
            input_name: input.name.clone(),
            source: source.id,
            source_name: source.name.clone(),
            tap: SourceTap::AfterEffects,
        })
        .into_iter()
        .collect();
    UiEffect {
        reconfiguration_failed: false,
        latency_samples: Some(latency_samples),
        id: EffectId::new(),
        effect_type,
        bypass: false,
        params: Vec::new(),
        descriptors: if plugin_ref.is_some() {
            &[]
        } else {
            builtin.param_descriptors()
        },
        plugin_name: plugin_ref.as_ref().map(|device| device.name.clone()),
        has_plugin_gui,
        plugin_ref,
        sidechains,
        inactive_sidechains: Vec::new(),
        external_inputs,
    }
}

impl Gallery {
    fn new() -> Self {
        let kick = ProjectTrack::new(TrackId::new(), "Kick".into(), 1);
        let effects = vec![
            effect(EffectType::Compressor, None, false, true, Some(&kick), 0),
            effect(
                EffectType::Compressor,
                Some(plugin("ZeroComp", "clap")),
                true,
                true,
                None,
                0,
            ),
            effect(
                EffectType::Compressor,
                Some(plugin("ZL Compressor", "vst3")),
                true,
                true,
                Some(&kick),
                507,
            ),
            effect(
                EffectType::Compressor,
                Some(plugin("Long Named Linear Phase Equalizer", "clap")),
                true,
                false,
                None,
                2048,
            ),
            effect(
                EffectType::Compressor,
                Some(plugin("Headless", "clap")),
                false,
                false,
                None,
                0,
            ),
        ];
        let mut devices = DevicesState::default();
        for effect in &effects {
            for input in &effect.external_inputs {
                devices.sidechain_choices.insert(
                    (effect.id, input.id),
                    vec![vibez_core::routing::InputSourceChoice {
                        source: kick.id,
                        taps: vec![
                            SourceTap::BeforeEffects,
                            SourceTap::AfterEffects,
                            SourceTap::AfterFader,
                        ],
                    }],
                );
                devices
                    .sidechain_meters
                    .insert((effect.id, input.id), (0.4, 0.3));
            }
        }
        Self {
            receiver: TrackId::new(),
            sources: vec![kick],
            effects,
            devices,
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let color = th::track_color(3);
        let mut cards = row![crate::widgets::device_chrome::plugin_instrument_card(
            self.receiver,
            "Surge XT",
            Some("clap"),
            true,
            crate::domains::device_latency::latency_label(Some(0), 48_000),
            color,
        )]
        .spacing(6);
        for effect in &self.effects {
            let sidechain =
                view_sidechain(self.receiver, effect, &self.sources, &[], &self.devices);
            cards = cards.push(view_effect_slot(
                self.receiver,
                effect,
                color,
                None,
                sidechain,
                48_000,
            ));
        }
        container(column![text("Bass - Devices").size(13).color(th::text()), cards].spacing(6))
            .padding(8)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::bg_dark().into()),
                ..Default::default()
            })
            .into()
    }
}

pub fn run() -> iced::Result {
    iced::application(
        "Device card gallery",
        |_: &mut Gallery, _: Message| {},
        Gallery::view,
    )
    .theme(|_| th::vibez_theme())
    .antialiasing(true)
    .window_size((1900.0, 300.0))
    .font(crate::icons::ICON_FONT_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_MEDIUM_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_SEMIBOLD_BYTES)
    .font(crate::typography::PLEX_MONO_MEDIUM_BYTES)
    .font(crate::typography::PLEX_MONO_SEMIBOLD_BYTES)
    .run_with(|| (Gallery::new(), iced::Task::none()))
}
