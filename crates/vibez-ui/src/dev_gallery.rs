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

/// Native Gain at each width tier and section height it adapts to, with
/// meter readings and a level history standing in for live audio.
struct GainGallery {
    track: TrackId,
    effect: UiEffect,
    closed: crate::domains::gain::GainPanelState,
    open: crate::domains::gain::GainPanelState,
}

fn amplitude(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

fn gain_panel(advanced: bool, clip: bool) -> crate::domains::gain::GainPanelState {
    use crate::domains::gain::{GainHistoryPoint, GainPanelState};
    use vibez_engine::metering::MeterValues;
    let now = std::time::Instant::now();
    let levels = |offset: f32| MeterValues {
        peak_l: amplitude(-6.2 + offset),
        peak_r: amplitude(-6.8 + offset),
        rms_l: amplitude(-11.0 + offset),
        rms_r: amplitude(-11.6 + offset),
    };
    let history = (0..200)
        .map(|step| {
            let seconds = step as f32 * 0.05;
            let swell = (seconds * 1.7).sin() * 1.5 + (seconds * 5.3).sin() * 0.8;
            GainHistoryPoint {
                at: now - std::time::Duration::from_secs_f32(10.0 - seconds),
                peak: amplitude(-7.0 + swell),
                rms: amplitude(-12.5 + swell * 0.6),
            }
        })
        .collect();
    GainPanelState {
        advanced,
        input: levels(0.0),
        output: levels(0.1),
        clip_latched: clip,
        history,
        last_meter_at: Some(now),
        ..Default::default()
    }
}

impl GainGallery {
    fn new() -> Self {
        let mut effect = effect(EffectType::Gain, None, false, false, None, 0);
        effect.params = vec![1.0, 0.0, 0.0, 0.0, 0.0];
        Self {
            track: TrackId::new(),
            effect,
            closed: gain_panel(false, false),
            open: gain_panel(true, true),
        }
    }

    fn scene<'a>(
        &'a self,
        label: &'static str,
        card_width: f32,
        section: (f32, f32),
        advanced: bool,
    ) -> Element<'a, Message> {
        let accent = th::track_color(0);
        let panel = if advanced { &self.open } else { &self.closed };
        let effect = &self.effect;
        let track = self.track;
        let body = iced::widget::responsive(move |size| {
            crate::widgets::gain_device::view(
                track,
                effect,
                accent,
                panel,
                crate::widgets::device_preset_bar::view(
                    track,
                    effect,
                    None,
                    accent,
                    size.width - 140.0,
                ),
                size,
            )
        });
        let card = view_effect_slot(
            track,
            effect,
            accent,
            Some(crate::widgets::effect_slot::DeviceBody {
                content: body.into(),
                width: card_width,
                padding: iced::Padding::ZERO,
            }),
            None,
            48_000,
        );
        column![
            text(label).size(11).color(th::text_dim()),
            container(row![card].height(Length::Fill))
                .padding(8)
                .width(section.0)
                .height(section.1)
                .style(|_theme: &Theme| container::Style {
                    background: Some(th::bg_dark().into()),
                    border: iced::Border {
                        color: th::border(),
                        width: 1.0,
                        radius: 0.0.into(),
                    },
                    ..Default::default()
                }),
        ]
        .spacing(4)
        .into()
    }

    fn view(&self) -> Element<'_, Message> {
        use crate::widgets::gain_device::{COMPACT_W, MAX_W, MEDIUM_W};
        container(
            column![
                row![
                    self.scene(
                        "Expanded, tall, Advanced open",
                        MAX_W,
                        (MAX_W + 16.0, 340.0),
                        true
                    ),
                    self.scene(
                        "Compact, normal",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 250.0),
                        false
                    ),
                ]
                .spacing(24),
                row![
                    self.scene("Medium, normal", MEDIUM_W, (MEDIUM_W + 16.0, 250.0), false),
                    self.scene(
                        "Medium, normal, Advanced open",
                        MEDIUM_W,
                        (MEDIUM_W + 16.0, 250.0),
                        true
                    ),
                ]
                .spacing(24),
                row![
                    self.scene(
                        "Medium, minimum height",
                        MEDIUM_W,
                        (MEDIUM_W + 16.0, 150.0),
                        false
                    ),
                    self.scene(
                        "Medium, minimum height, Advanced open",
                        MEDIUM_W,
                        (MEDIUM_W + 16.0, 150.0),
                        true
                    ),
                ]
                .spacing(24),
            ]
            .spacing(16),
        )
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_theme: &Theme| container::Style {
            background: Some(th::bg_surface().into()),
            ..Default::default()
        })
        .into()
    }
}

pub fn run_gain() -> iced::Result {
    iced::application(
        "Gain device gallery",
        |_: &mut GainGallery, _: Message| {},
        GainGallery::view,
    )
    .theme(|_| th::vibez_theme())
    .antialiasing(true)
    .window_size((1400.0, 1000.0))
    .font(crate::icons::ICON_FONT_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_MEDIUM_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_SEMIBOLD_BYTES)
    .font(crate::typography::PLEX_MONO_MEDIUM_BYTES)
    .font(crate::typography::PLEX_MONO_SEMIBOLD_BYTES)
    .run_with(|| (GainGallery::new(), iced::Task::none()))
}
