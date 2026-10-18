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

/// A device card as the device section would hold it, `section` wide and
/// tall, under a caption naming the case.
fn framed_card<'a>(
    label: &'static str,
    card_width: f32,
    section: (f32, f32),
    track: TrackId,
    effect: &'a UiEffect,
    accent: iced::Color,
    body: Element<'a, Message>,
) -> Element<'a, Message> {
    let card = view_effect_slot(
        track,
        effect,
        accent,
        Some(crate::widgets::effect_slot::DeviceBody {
            content: body,
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

/// Native Gain at each width tier and section height it adapts to, with
/// meter readings and a level history standing in for live audio.
struct GainGallery {
    track: TrackId,
    effect: UiEffect,
    closed: crate::domains::gain::GainPanelState,
    open: crate::domains::gain::GainPanelState,
}

fn gain_panel(advanced: bool, clip: bool) -> crate::domains::gain::GainPanelState {
    use crate::domains::gain::{GainHistoryPoint, GainPanelState};
    use vibez_engine::metering::MeterValues;
    let now = std::time::Instant::now();
    let levels = |offset: f32| MeterValues {
        peak_l: vibez_core::level::db_to_amplitude(-6.2 + offset),
        peak_r: vibez_core::level::db_to_amplitude(-6.8 + offset),
        rms_l: vibez_core::level::db_to_amplitude(-11.0 + offset),
        rms_r: vibez_core::level::db_to_amplitude(-11.6 + offset),
    };
    let history = (0..200)
        .map(|step| {
            let seconds = step as f32 * 0.05;
            let swell = (seconds * 1.7).sin() * 1.5 + (seconds * 5.3).sin() * 0.8;
            (
                now - std::time::Duration::from_secs_f32(10.0 - seconds),
                GainHistoryPoint {
                    peak: vibez_core::level::db_to_amplitude(-7.0 + swell),
                    rms: vibez_core::level::db_to_amplitude(-12.5 + swell * 0.6),
                },
            )
        })
        .collect();
    GainPanelState {
        advanced,
        input: levels(0.0),
        output: levels(0.1),
        clip_latched: clip,
        history,
        last_meter_at: Some(now),
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
                None,
                crate::widgets::device_preset_bar::view(
                    track,
                    effect,
                    None,
                    accent,
                    crate::widgets::gain_device::preset_bar_width(size),
                ),
                size,
            )
        });
        framed_card(
            label,
            card_width,
            section,
            track,
            effect,
            accent,
            body.into(),
        )
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

/// Native EQ at each width tier and section height it adapts to, with a
/// fixed spectrum standing in for live audio.
struct EqGallery {
    track: TrackId,
    effect: UiEffect,
    spectrum: crate::spectrum::SpectrumState,
    closed: crate::domains::eq::EqPanelState,
    open: crate::domains::eq::EqPanelState,
}

impl EqGallery {
    fn new() -> Self {
        use vibez_dsp::eq::param;
        let mut effect = effect(EffectType::Eq, None, false, false, None, 0);
        effect.params = effect.descriptors.iter().map(|d| d.default).collect();
        for (index, value) in [
            (param::GAIN[1], -1.5),
            (param::FREQUENCY[1], 350.0),
            (param::GAIN[2], 2.5),
            (param::FREQUENCY[2], 3200.0),
            (param::SHAPE[2], 0.8),
            (param::GAIN[3], -1.0),
            (param::FREQUENCY[3], 12_000.0),
        ] {
            effect.params[index] = value;
        }
        let mut spectrum = crate::spectrum::SpectrumState::default();
        let bins = spectrum.bins.len() as f32;
        for (bin, level) in spectrum.bins.iter_mut().enumerate() {
            let position = bin as f32 / bins;
            *level = -18.0 - position * 30.0 + (position * 41.0).sin() * 4.0;
        }
        spectrum.active = true;
        let level = |db: f32| vibez_engine::metering::MeterValues {
            peak_l: vibez_core::level::db_to_amplitude(db),
            peak_r: vibez_core::level::db_to_amplitude(db - 0.6),
            rms_l: vibez_core::level::db_to_amplitude(db - 5.0),
            rms_r: vibez_core::level::db_to_amplitude(db - 5.6),
        };
        let closed = crate::domains::eq::EqPanelState {
            band: 2,
            input: level(-6.2),
            output: level(-6.1),
            ..Default::default()
        };
        Self {
            track: TrackId::new(),
            effect,
            spectrum,
            open: crate::domains::eq::EqPanelState {
                advanced: true,
                ..closed.clone()
            },
            closed,
        }
    }

    fn scene<'a>(
        &'a self,
        label: &'static str,
        card_width: f32,
        section: (f32, f32),
        panel: &'a crate::domains::eq::EqPanelState,
    ) -> Element<'a, Message> {
        let accent = th::track_color(0);
        let effect = &self.effect;
        let track = self.track;
        let spectrum = &self.spectrum;
        let body = iced::widget::responsive(move |size| {
            crate::widgets::eq_device::EqView {
                track_id: track,
                effect,
                accent,
                panel,
                entry: None,
                spectrum: Some(spectrum),
                sample_rate: 48_000.0,
            }
            .view(
                crate::widgets::device_preset_bar::view(
                    track,
                    effect,
                    None,
                    accent,
                    crate::widgets::eq_device::preset_bar_width(size),
                ),
                size,
            )
        });
        framed_card(
            label,
            card_width,
            section,
            track,
            effect,
            accent,
            body.into(),
        )
    }

    fn view(&self) -> Element<'_, Message> {
        use crate::widgets::eq_device::{COMPACT_W, FULL_W, MEDIUM_W};
        let (closed, open) = (&self.closed, &self.open);
        container(
            column![
                row![self.scene("Full, tall", FULL_W, (FULL_W + 16.0, 330.0), closed)],
                row![
                    self.scene("Medium, normal", MEDIUM_W, (MEDIUM_W + 16.0, 250.0), closed),
                    self.scene(
                        "Full, minimum height",
                        FULL_W,
                        (FULL_W + 16.0, 150.0),
                        closed
                    ),
                ]
                .spacing(24),
                row![
                    self.scene(
                        "Compact, tall",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 300.0),
                        closed
                    ),
                    self.scene(
                        "Compact, minimum height",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 150.0),
                        closed
                    ),
                    self.scene("Full, Advanced open", FULL_W, (FULL_W + 16.0, 300.0), open),
                ]
                .spacing(24),
                row![
                    self.scene("Full, shortest", FULL_W, (FULL_W + 16.0, 130.0), closed),
                    self.scene(
                        "Compact, shortest, Advanced open",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 130.0),
                        open
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

pub fn run_eq() -> iced::Result {
    iced::application(
        "EQ device gallery",
        |_: &mut EqGallery, _: Message| {},
        EqGallery::view,
    )
    .theme(|_| th::vibez_theme())
    .antialiasing(true)
    .window_size((1900.0, 1260.0))
    .font(crate::icons::ICON_FONT_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_MEDIUM_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_SEMIBOLD_BYTES)
    .font(crate::typography::PLEX_MONO_MEDIUM_BYTES)
    .font(crate::typography::PLEX_MONO_SEMIBOLD_BYTES)
    .run_with(|| (EqGallery::new(), iced::Task::none()))
}

/// Native Compressor at each width tier and section height it adapts to,
/// with levels and a reduction history standing in for live audio.
struct CompressorGallery {
    track: TrackId,
    effect: UiEffect,
    closed: crate::domains::compressor::CompressorPanelState,
    open: crate::domains::compressor::CompressorPanelState,
}

impl CompressorGallery {
    fn new() -> Self {
        use crate::domains::compressor::CompressorPanelState;
        let mut effect = effect(EffectType::Compressor, None, false, false, None, 0);
        effect.params = effect.descriptors.iter().map(|d| d.default).collect();
        let now = std::time::Instant::now();
        let level = |db: f32| vibez_engine::metering::MeterValues {
            peak_l: vibez_core::level::db_to_amplitude(db),
            peak_r: vibez_core::level::db_to_amplitude(db - 0.5),
            rms_l: vibez_core::level::db_to_amplitude(db - 6.0),
            rms_r: vibez_core::level::db_to_amplitude(db - 6.5),
        };
        let history = (0..200)
            .map(|step| {
                let seconds = step as f32 * 0.025;
                let swell =
                    3.0 + (seconds * 4.1).sin() * 1.2 + (seconds * 11.0).sin().max(0.0) * 2.5;
                (
                    now - std::time::Duration::from_secs_f32(5.0 - seconds),
                    swell.max(0.0),
                )
            })
            .collect();
        let closed = CompressorPanelState {
            input: level(-9.0),
            output: level(-10.0),
            reduction_db: 3.2,
            history,
            last_meter_at: Some(now),
            ..Default::default()
        };
        Self {
            track: TrackId::new(),
            effect,
            open: CompressorPanelState {
                advanced: true,
                ..closed.clone()
            },
            closed,
        }
    }

    fn scene<'a>(
        &'a self,
        label: &'static str,
        card_width: f32,
        section: (f32, f32),
        panel: &'a crate::domains::compressor::CompressorPanelState,
    ) -> Element<'a, Message> {
        let accent = th::track_color(0);
        let effect = &self.effect;
        let track = self.track;
        let body = iced::widget::responsive(move |size| {
            crate::widgets::compressor_device::CompressorView {
                track_id: track,
                effect,
                accent,
                panel,
                entry: None,
            }
            .view(
                crate::widgets::device_preset_bar::view(
                    track,
                    effect,
                    None,
                    accent,
                    crate::widgets::compressor_device::preset_bar_width(size),
                ),
                size,
            )
        });
        framed_card(
            label,
            card_width,
            section,
            track,
            effect,
            accent,
            body.into(),
        )
    }

    fn view(&self) -> Element<'_, Message> {
        use crate::widgets::compressor_device::{COMPACT_W, FULL_W, MEDIUM_W};
        let (closed, open) = (&self.closed, &self.open);
        container(
            column![
                row![
                    self.scene("Full, tall", FULL_W, (FULL_W + 16.0, 330.0), closed),
                    self.scene(
                        "Compact, tall",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 330.0),
                        closed
                    ),
                ]
                .spacing(24),
                row![
                    self.scene("Full, Advanced open", FULL_W, (FULL_W + 16.0, 400.0), open),
                    self.scene("Medium, normal", MEDIUM_W, (MEDIUM_W + 16.0, 250.0), closed),
                ]
                .spacing(24),
                row![
                    self.scene(
                        "Full, minimum height",
                        FULL_W,
                        (FULL_W + 16.0, 150.0),
                        closed
                    ),
                    self.scene(
                        "Compact, Advanced open",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 250.0),
                        open
                    ),
                    self.scene("Full, shortest", FULL_W, (FULL_W + 16.0, 110.0), closed),
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

pub fn run_compressor() -> iced::Result {
    iced::application(
        "Compressor device gallery",
        |_: &mut CompressorGallery, _: Message| {},
        CompressorGallery::view,
    )
    .theme(|_| th::vibez_theme())
    .antialiasing(true)
    .window_size((1900.0, 1260.0))
    .font(crate::icons::ICON_FONT_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_MEDIUM_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_SEMIBOLD_BYTES)
    .font(crate::typography::PLEX_MONO_MEDIUM_BYTES)
    .font(crate::typography::PLEX_MONO_SEMIBOLD_BYTES)
    .run_with(|| (CompressorGallery::new(), iced::Task::none()))
}

/// Native Filter at each width tier and section height it adapts to, on
/// the Gentle Sweep preset as in the concept, with an output level standing
/// in for live audio.
struct FilterGallery {
    track: TrackId,
    effect: UiEffect,
    closed: crate::domains::filter::FilterPanelState,
    open: crate::domains::filter::FilterPanelState,
    modulation: crate::domains::filter::FilterPanelState,
}

impl FilterGallery {
    fn new() -> Self {
        use crate::domains::filter::{FilterPanelState, Page};
        use vibez_dsp::filter::param;
        let mut effect = effect(EffectType::Filter, None, false, false, None, 0);
        effect.params = effect.descriptors.iter().map(|d| d.default).collect();
        effect.params[param::CUTOFF] = 1_200.0;
        effect.params[param::SLOPE] = 1.0;
        effect.params[param::MOD_SOURCE] = 1.0;
        effect.params[param::SYNC] = 3.0;
        effect.params[param::DEPTH] = 1.1;
        let level = |db: f32| vibez_engine::metering::MeterValues {
            peak_l: vibez_core::level::db_to_amplitude(db),
            peak_r: vibez_core::level::db_to_amplitude(db - 0.5),
            rms_l: vibez_core::level::db_to_amplitude(db - 6.0),
            rms_r: vibez_core::level::db_to_amplitude(db - 6.5),
        };
        let closed = FilterPanelState {
            output: level(-14.0),
            last_meter_at: Some(std::time::Instant::now()),
            ..Default::default()
        };
        Self {
            track: TrackId::new(),
            effect,
            open: FilterPanelState {
                advanced: true,
                ..closed.clone()
            },
            modulation: FilterPanelState {
                page: Page::Modulation,
                ..closed.clone()
            },
            closed,
        }
    }

    fn scene<'a>(
        &'a self,
        label: &'static str,
        card_width: f32,
        section: (f32, f32),
        panel: &'a crate::domains::filter::FilterPanelState,
    ) -> Element<'a, Message> {
        let accent = th::track_color(0);
        let effect = &self.effect;
        let track = self.track;
        let body = iced::widget::responsive(move |size| {
            crate::widgets::filter_device::FilterView {
                track_id: track,
                effect,
                accent,
                panel,
                entry: None,
                sample_rate: 48_000.0,
            }
            .view(
                crate::widgets::device_preset_bar::view(
                    track,
                    effect,
                    None,
                    accent,
                    crate::widgets::filter_device::preset_bar_width(size),
                ),
                size,
            )
        });
        framed_card(
            label,
            card_width,
            section,
            track,
            effect,
            accent,
            body.into(),
        )
    }

    fn view(&self) -> Element<'_, Message> {
        use crate::widgets::filter_device::{COMPACT_W, FULL_W, MEDIUM_W};
        let (closed, open, modulation) = (&self.closed, &self.open, &self.modulation);
        container(
            column![
                row![
                    self.scene("Full, tall", FULL_W, (FULL_W + 16.0, 330.0), closed),
                    self.scene(
                        "Compact, tall",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 330.0),
                        closed
                    ),
                    self.scene("Medium, normal", MEDIUM_W, (MEDIUM_W + 16.0, 330.0), closed),
                ]
                .spacing(24),
                row![
                    self.scene("Full, Advanced open", FULL_W, (FULL_W + 16.0, 400.0), open),
                    self.scene(
                        "Compact, Modulation",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 250.0),
                        modulation
                    ),
                    self.scene(
                        "Medium, Advanced open",
                        MEDIUM_W,
                        (MEDIUM_W + 16.0, 250.0),
                        open
                    ),
                ]
                .spacing(24),
                row![
                    self.scene("Full, shortest", FULL_W, (FULL_W + 16.0, 110.0), closed),
                    self.scene(
                        "Compact, short",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 150.0),
                        closed
                    ),
                    self.scene(
                        "Compact, shortest",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 110.0),
                        modulation
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

pub fn run_filter() -> iced::Result {
    iced::application(
        "Filter device gallery",
        |_: &mut FilterGallery, _: Message| {},
        FilterGallery::view,
    )
    .theme(|_| th::vibez_theme())
    .antialiasing(true)
    .window_size((1900.0, 1260.0))
    .font(crate::icons::ICON_FONT_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_MEDIUM_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_SEMIBOLD_BYTES)
    .font(crate::typography::PLEX_MONO_MEDIUM_BYTES)
    .font(crate::typography::PLEX_MONO_SEMIBOLD_BYTES)
    .run_with(|| (FilterGallery::new(), iced::Task::none()))
}

/// Native Gate at each width tier and section height it adapts to, on the
/// Tight Drums preset as in the concept, with a kick-like detector history
/// and levels standing in for live audio.
struct GateGallery {
    track: TrackId,
    effect: UiEffect,
    closed: crate::domains::gate::GatePanelState,
    open: crate::domains::gate::GatePanelState,
}

impl GateGallery {
    fn new() -> Self {
        use crate::domains::gate::{GatePanelState, GateReading, HISTORY};
        use vibez_dsp::gate::param;
        let mut effect = effect(EffectType::Gate, None, false, false, None, 0);
        effect.params = effect.descriptors.iter().map(|d| d.default).collect();
        effect.params[param::THRESHOLD] = -32.0;
        effect.params[param::HOLD] = 60.0;
        effect.params[param::RELEASE] = 120.0;
        effect.params[param::HYSTERESIS] = 6.0;
        effect.params[param::RANGE_ENABLED] = 1.0;
        let now = std::time::Instant::now();
        let level = |db: f32| vibez_engine::metering::MeterValues {
            peak_l: vibez_core::level::db_to_amplitude(db),
            peak_r: vibez_core::level::db_to_amplitude(db - 0.5),
            rms_l: vibez_core::level::db_to_amplitude(db - 6.0),
            rms_r: vibez_core::level::db_to_amplitude(db - 6.5),
        };
        let span = HISTORY.as_secs_f32();
        let history = (0..240)
            .map(|step| {
                let seconds = step as f32 * span / 240.0;
                let beat = (seconds * 2.0).fract();
                let hiss = (seconds * 97.0).sin() * 3.0 + (seconds * 41.0).sin() * 2.0;
                let level_db = (-6.0 - beat * 90.0).max(-44.0 + hiss);
                let open = beat < 0.25;
                (
                    now - std::time::Duration::from_secs_f32(span - seconds),
                    GateReading {
                        level_db,
                        open,
                        reduction_db: if open { 0.0 } else { 60.0 },
                    },
                )
            })
            .collect();
        let closed = GatePanelState {
            input: level(-12.0),
            output: level(-13.0),
            phase: vibez_core::effect::GatePhase::Open,
            history,
            last_meter_at: Some(now),
            ..Default::default()
        };
        Self {
            track: TrackId::new(),
            effect,
            open: GatePanelState {
                advanced: true,
                ..closed.clone()
            },
            closed,
        }
    }

    fn scene<'a>(
        &'a self,
        label: &'static str,
        card_width: f32,
        section: (f32, f32),
        panel: &'a crate::domains::gate::GatePanelState,
    ) -> Element<'a, Message> {
        let accent = th::track_color(0);
        let effect = &self.effect;
        let track = self.track;
        let body = iced::widget::responsive(move |size| {
            crate::widgets::gate_device::GateView {
                track_id: track,
                effect,
                accent,
                panel,
                entry: None,
            }
            .view(
                crate::widgets::device_preset_bar::view(
                    track,
                    effect,
                    None,
                    accent,
                    crate::widgets::gate_device::preset_bar_width(size),
                ),
                size,
            )
        });
        framed_card(
            label,
            card_width,
            section,
            track,
            effect,
            accent,
            body.into(),
        )
    }

    fn view(&self) -> Element<'_, Message> {
        use crate::widgets::gate_device::{COMPACT_W, FULL_W};
        let wide = 900.0;
        let (closed, open) = (&self.closed, &self.open);
        container(
            column![
                row![
                    self.scene("Wide, tall", wide, (wide + 16.0, 330.0), closed),
                    self.scene(
                        "Compact, tall",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 330.0),
                        closed
                    ),
                ]
                .spacing(24),
                row![
                    self.scene("Wide, Advanced open", wide, (wide + 16.0, 400.0), open),
                    self.scene(
                        "Compact, Advanced open",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 400.0),
                        open
                    ),
                ]
                .spacing(24),
                row![
                    self.scene("Full, shortest", FULL_W, (FULL_W + 16.0, 110.0), closed),
                    self.scene("Compact, short", COMPACT_W, (COMPACT_W + 16.0, 150.0), open),
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

pub fn run_gate() -> iced::Result {
    iced::application(
        "Gate device gallery",
        |_: &mut GateGallery, _: Message| {},
        GateGallery::view,
    )
    .theme(|_| th::vibez_theme())
    .antialiasing(true)
    .window_size((1900.0, 1260.0))
    .font(crate::icons::ICON_FONT_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_MEDIUM_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_SEMIBOLD_BYTES)
    .font(crate::typography::PLEX_MONO_MEDIUM_BYTES)
    .font(crate::typography::PLEX_MONO_SEMIBOLD_BYTES)
    .run_with(|| (GateGallery::new(), iced::Task::none()))
}

/// Native Drive at each width tier and section height it adapts to, on the
/// Warm Edge preset as in the concept, with levels standing in for live
/// audio.
struct DriveGallery {
    track: TrackId,
    effect: UiEffect,
    closed: crate::domains::device_card::CardPanelState,
    open: crate::domains::device_card::CardPanelState,
}

impl DriveGallery {
    fn new() -> Self {
        use crate::domains::device_card::CardPanelState;
        use vibez_dsp::drive::{character, param};
        let mut effect = effect(EffectType::Drive, None, false, false, None, 0);
        effect.params = effect.descriptors.iter().map(|d| d.default).collect();
        effect.params[param::AMOUNT] =
            vibez_dsp::drive::amount_for_gain(vibez_core::level::db_to_amplitude(6.0));
        effect.params[param::TONE] = 0.55;
        effect.params[param::OUTPUT] = vibez_core::level::db_to_amplitude(-3.0);
        effect.params[param::MIX] = 0.8;
        effect.params[param::CHARACTER] = character::SOFT as f32;
        let level = |db: f32| vibez_engine::metering::MeterValues {
            peak_l: vibez_core::level::db_to_amplitude(db),
            peak_r: vibez_core::level::db_to_amplitude(db - 0.5),
            rms_l: vibez_core::level::db_to_amplitude(db - 6.0),
            rms_r: vibez_core::level::db_to_amplitude(db - 6.5),
        };
        let closed = CardPanelState {
            input: level(-9.0),
            output: level(-7.0),
            last_meter_at: Some(std::time::Instant::now()),
            ..Default::default()
        };
        Self {
            track: TrackId::new(),
            effect,
            open: CardPanelState {
                advanced: true,
                ..closed.clone()
            },
            closed,
        }
    }

    fn scene<'a>(
        &'a self,
        label: &'static str,
        card_width: f32,
        section: (f32, f32),
        panel: &'a crate::domains::device_card::CardPanelState,
    ) -> Element<'a, Message> {
        let accent = th::track_color(0);
        let effect = &self.effect;
        let track = self.track;
        let body = iced::widget::responsive(move |size| {
            crate::widgets::drive_device::DriveView {
                track_id: track,
                effect,
                accent,
                panel,
                entry: None,
            }
            .view(
                crate::widgets::device_preset_bar::view(
                    track,
                    effect,
                    None,
                    accent,
                    crate::widgets::drive_device::preset_bar_width(size),
                ),
                size,
            )
        });
        framed_card(
            label,
            card_width,
            section,
            track,
            effect,
            accent,
            body.into(),
        )
    }

    fn view(&self) -> Element<'_, Message> {
        use crate::widgets::drive_device::{COMPACT_W, WAVEFORM_W};
        let wide = 900.0;
        let (closed, open) = (&self.closed, &self.open);
        container(
            column![
                row![
                    self.scene("Wide, tall", wide, (wide + 16.0, 330.0), closed),
                    self.scene(
                        "Compact, tall",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 330.0),
                        closed
                    ),
                ]
                .spacing(24),
                row![
                    self.scene("Wide, Advanced open", wide, (wide + 16.0, 400.0), open),
                    self.scene(
                        "Medium, Advanced open",
                        WAVEFORM_W,
                        (WAVEFORM_W + 16.0, 400.0),
                        open
                    ),
                ]
                .spacing(24),
                row![
                    self.scene("Wide, shortest", wide, (wide + 16.0, 110.0), closed),
                    self.scene("Compact, short", COMPACT_W, (COMPACT_W + 16.0, 150.0), open),
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

pub fn run_drive() -> iced::Result {
    iced::application(
        "Drive device gallery",
        |_: &mut DriveGallery, _: Message| {},
        DriveGallery::view,
    )
    .theme(|_| th::vibez_theme())
    .antialiasing(true)
    .window_size((1900.0, 1260.0))
    .font(crate::icons::ICON_FONT_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_MEDIUM_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_SEMIBOLD_BYTES)
    .font(crate::typography::PLEX_MONO_MEDIUM_BYTES)
    .font(crate::typography::PLEX_MONO_SEMIBOLD_BYTES)
    .run_with(|| (DriveGallery::new(), iced::Task::none()))
}

/// Native Bitcrush at each width tier and section height it adapts to, set
/// like the concept's Crunchy 12 with levels standing in for live audio.
struct BitcrushGallery {
    track: TrackId,
    effect: UiEffect,
    closed: crate::domains::device_card::CardPanelState,
    open: crate::domains::device_card::CardPanelState,
}

impl BitcrushGallery {
    fn new() -> Self {
        use crate::domains::device_card::CardPanelState;
        use vibez_dsp::bitcrush::param;
        let mut effect = effect(EffectType::Bitcrush, None, false, false, None, 0);
        effect.params = effect.descriptors.iter().map(|d| d.default).collect();
        effect.params[param::BITS] = 12.0;
        effect.params[param::SR_REDUCE] = 4.0;
        effect.params[param::DRIVE_DB] = 2.0;
        effect.params[param::MIX] = 0.65;
        let level = |db: f32| vibez_engine::metering::MeterValues {
            peak_l: vibez_core::level::db_to_amplitude(db),
            peak_r: vibez_core::level::db_to_amplitude(db - 0.5),
            rms_l: vibez_core::level::db_to_amplitude(db - 6.0),
            rms_r: vibez_core::level::db_to_amplitude(db - 6.5),
        };
        let closed = CardPanelState {
            input: level(-2.0),
            output: level(-3.0),
            last_meter_at: Some(std::time::Instant::now()),
            ..Default::default()
        };
        Self {
            track: TrackId::new(),
            effect,
            open: CardPanelState {
                advanced: true,
                ..closed.clone()
            },
            closed,
        }
    }

    fn scene<'a>(
        &'a self,
        label: &'static str,
        card_width: f32,
        section: (f32, f32),
        panel: &'a crate::domains::device_card::CardPanelState,
    ) -> Element<'a, Message> {
        let accent = th::track_color(0);
        let effect = &self.effect;
        let track = self.track;
        let body = iced::widget::responsive(move |size| {
            crate::widgets::bitcrush_device::BitcrushView {
                track_id: track,
                effect,
                accent,
                panel,
                entry: None,
                host_rate: 44_100,
            }
            .view(
                crate::widgets::device_preset_bar::view(
                    track,
                    effect,
                    None,
                    accent,
                    crate::widgets::bitcrush_device::preset_bar_width(size),
                ),
                size,
            )
        });
        framed_card(
            label,
            card_width,
            section,
            track,
            effect,
            accent,
            body.into(),
        )
    }

    fn view(&self) -> Element<'_, Message> {
        use crate::widgets::bitcrush_device::COMPACT_W;
        let wide = 900.0;
        let (closed, open) = (&self.closed, &self.open);
        container(
            column![
                row![
                    self.scene("Wide, tall", wide, (wide + 16.0, 330.0), closed),
                    self.scene(
                        "Compact, tall",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 330.0),
                        closed
                    ),
                ]
                .spacing(24),
                row![
                    self.scene("Wide, Advanced open", wide, (wide + 16.0, 400.0), open),
                    self.scene(
                        "Compact, Advanced open",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 400.0),
                        open
                    ),
                ]
                .spacing(24),
                row![
                    self.scene("Wide, shortest", wide, (wide + 16.0, 110.0), closed),
                    self.scene("Compact, short", COMPACT_W, (COMPACT_W + 16.0, 150.0), open),
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

pub fn run_bitcrush() -> iced::Result {
    iced::application(
        "Bitcrush device gallery",
        |_: &mut BitcrushGallery, _: Message| {},
        BitcrushGallery::view,
    )
    .theme(|_| th::vibez_theme())
    .antialiasing(true)
    .window_size((1900.0, 1260.0))
    .font(crate::icons::ICON_FONT_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_MEDIUM_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_SEMIBOLD_BYTES)
    .font(crate::typography::PLEX_MONO_MEDIUM_BYTES)
    .font(crate::typography::PLEX_MONO_SEMIBOLD_BYTES)
    .run_with(|| (BitcrushGallery::new(), iced::Task::none()))
}

/// Native Delay at each width tier and section height it adapts to, on the
/// Dotted Bounce preset as in the concept, with an output level standing in
/// for live audio.
struct DelayGallery {
    track: TrackId,
    effect: UiEffect,
    closed: crate::domains::device_card::CardPanelState,
    open: crate::domains::device_card::CardPanelState,
}

impl DelayGallery {
    fn new() -> Self {
        use crate::domains::device_card::CardPanelState;
        use vibez_dsp::delay::param;
        let mut effect = effect(EffectType::Delay, None, false, false, None, 0);
        effect.params = effect.descriptors.iter().map(|d| d.default).collect();
        effect.params[param::SYNC] = 1.0;
        effect.params[param::LEFT_DIVISION] = 3.0;
        effect.params[param::LEFT_MODIFIER] = 1.0;
        effect.params[param::RIGHT_DIVISION] = 3.0;
        effect.params[param::RIGHT_MODIFIER] = 1.0;
        effect.params[param::FEEDBACK] = 0.35;
        effect.params[param::MIX] = 0.22;
        effect.params[param::FEEDBACK_HP_ENABLED] = 1.0;
        effect.params[param::FEEDBACK_LP_ENABLED] = 1.0;
        effect.params[param::DUCK] = 0.2;
        let level = |db: f32| vibez_engine::metering::MeterValues {
            peak_l: vibez_core::level::db_to_amplitude(db),
            peak_r: vibez_core::level::db_to_amplitude(db - 0.5),
            rms_l: vibez_core::level::db_to_amplitude(db - 6.0),
            rms_r: vibez_core::level::db_to_amplitude(db - 6.5),
        };
        let closed = CardPanelState {
            input: level(-2.0),
            output: level(-3.0),
            last_meter_at: Some(std::time::Instant::now()),
            ..Default::default()
        };
        Self {
            track: TrackId::new(),
            effect,
            open: CardPanelState {
                advanced: true,
                ..closed.clone()
            },
            closed,
        }
    }

    fn scene<'a>(
        &'a self,
        label: &'static str,
        card_width: f32,
        section: (f32, f32),
        panel: &'a crate::domains::device_card::CardPanelState,
    ) -> Element<'a, Message> {
        let accent = th::track_color(0);
        let effect = &self.effect;
        let track = self.track;
        let body = iced::widget::responsive(move |size| {
            crate::widgets::delay_device::DelayView {
                track_id: track,
                effect,
                accent,
                panel,
                entry: None,
                bpm: 120.0,
            }
            .view(
                crate::widgets::device_preset_bar::view(
                    track,
                    effect,
                    None,
                    accent,
                    crate::widgets::delay_device::preset_bar_width(size),
                ),
                size,
            )
        });
        framed_card(
            label,
            card_width,
            section,
            track,
            effect,
            accent,
            body.into(),
        )
    }

    fn view(&self) -> Element<'_, Message> {
        use crate::widgets::delay_device::{COMPACT_W, FULL_W};
        let wide = 900.0;
        let (closed, open) = (&self.closed, &self.open);
        container(
            column![
                row![
                    self.scene("Wide, tall", wide, (wide + 16.0, 330.0), closed),
                    self.scene(
                        "Compact, tall",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 330.0),
                        closed
                    ),
                ]
                .spacing(24),
                row![
                    self.scene("Wide, Advanced open", wide, (wide + 16.0, 400.0), open),
                    self.scene(
                        "Compact, Advanced open",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 400.0),
                        open
                    ),
                ]
                .spacing(24),
                row![
                    self.scene("Full, shortest", FULL_W, (FULL_W + 16.0, 110.0), closed),
                    self.scene("Compact, short", COMPACT_W, (COMPACT_W + 16.0, 150.0), open),
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

pub fn run_delay() -> iced::Result {
    iced::application(
        "Delay device gallery",
        |_: &mut DelayGallery, _: Message| {},
        DelayGallery::view,
    )
    .theme(|_| th::vibez_theme())
    .antialiasing(true)
    .window_size((1900.0, 1260.0))
    .font(crate::icons::ICON_FONT_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_MEDIUM_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_SEMIBOLD_BYTES)
    .font(crate::typography::PLEX_MONO_MEDIUM_BYTES)
    .font(crate::typography::PLEX_MONO_SEMIBOLD_BYTES)
    .run_with(|| (DelayGallery::new(), iced::Task::none()))
}

/// Native Echo at each width tier and section height it adapts to, on its
/// defaults (the concept's Worn Tape), with an output level standing in for
/// live audio.
struct EchoGallery {
    track: TrackId,
    effect: UiEffect,
    closed: crate::domains::device_card::CardPanelState,
    open: crate::domains::device_card::CardPanelState,
}

impl EchoGallery {
    fn new() -> Self {
        use crate::domains::device_card::CardPanelState;
        let mut effect = effect(EffectType::Echo, None, false, false, None, 0);
        effect.params = effect.descriptors.iter().map(|d| d.default).collect();
        let level = |db: f32| vibez_engine::metering::MeterValues {
            peak_l: vibez_core::level::db_to_amplitude(db),
            peak_r: vibez_core::level::db_to_amplitude(db - 0.5),
            rms_l: vibez_core::level::db_to_amplitude(db - 6.0),
            rms_r: vibez_core::level::db_to_amplitude(db - 6.5),
        };
        let closed = CardPanelState {
            input: level(-4.0),
            output: level(-6.0),
            last_meter_at: Some(std::time::Instant::now()),
            ..Default::default()
        };
        Self {
            track: TrackId::new(),
            effect,
            open: CardPanelState {
                advanced: true,
                ..closed.clone()
            },
            closed,
        }
    }

    fn scene<'a>(
        &'a self,
        label: &'static str,
        card_width: f32,
        section: (f32, f32),
        panel: &'a crate::domains::device_card::CardPanelState,
    ) -> Element<'a, Message> {
        let accent = th::track_color(0);
        let effect = &self.effect;
        let track = self.track;
        let body = iced::widget::responsive(move |size| {
            crate::widgets::echo_device::EchoView {
                track_id: track,
                effect,
                accent,
                panel,
                entry: None,
                bpm: 120.0,
            }
            .view(
                crate::widgets::device_preset_bar::view(
                    track,
                    effect,
                    None,
                    accent,
                    crate::widgets::echo_device::preset_bar_width(size),
                ),
                size,
            )
        });
        framed_card(
            label,
            card_width,
            section,
            track,
            effect,
            accent,
            body.into(),
        )
    }

    fn view(&self) -> Element<'_, Message> {
        use crate::widgets::echo_device::{COMPACT_W, FULL_W, MEDIUM_W};
        let wide = 1_100.0;
        let (closed, open) = (&self.closed, &self.open);
        container(
            column![
                self.scene("Expanded, Advanced open", wide, (wide + 16.0, 400.0), open),
                row![
                    self.scene("Medium", MEDIUM_W + 60.0, (MEDIUM_W + 76.0, 330.0), closed),
                    self.scene(
                        "Compact, Advanced open",
                        COMPACT_W + 40.0,
                        (COMPACT_W + 56.0, 400.0),
                        open
                    ),
                ]
                .spacing(24),
                row![
                    self.scene("Full, shortest", FULL_W, (FULL_W + 16.0, 110.0), closed),
                    self.scene("Compact, short", COMPACT_W, (COMPACT_W + 16.0, 170.0), open),
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

pub fn run_echo() -> iced::Result {
    iced::application(
        "Echo device gallery",
        |_: &mut EchoGallery, _: Message| {},
        EchoGallery::view,
    )
    .theme(|_| th::vibez_theme())
    .antialiasing(true)
    .window_size((1900.0, 1260.0))
    .font(crate::icons::ICON_FONT_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_MEDIUM_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_SEMIBOLD_BYTES)
    .font(crate::typography::PLEX_MONO_MEDIUM_BYTES)
    .font(crate::typography::PLEX_MONO_SEMIBOLD_BYTES)
    .run_with(|| (EchoGallery::new(), iced::Task::none()))
}

/// Native Saturator at each width tier and section height it adapts to, on
/// its defaults, with an input level standing in for live audio, and at the
/// height the default detail panel gives it.
struct SaturatorGallery {
    track: TrackId,
    effect: UiEffect,
    closed: crate::domains::device_card::CardPanelState,
    open: crate::domains::device_card::CardPanelState,
    spectrum: crate::domains::device_card::CardPanelState,
}

impl SaturatorGallery {
    fn new() -> Self {
        use crate::domains::device_card::CardPanelState;
        let mut effect = effect(EffectType::Saturator, None, false, false, None, 0);
        effect.params = effect.descriptors.iter().map(|d| d.default).collect();
        let level = |db: f32| vibez_engine::metering::MeterValues {
            peak_l: vibez_core::level::db_to_amplitude(db),
            peak_r: vibez_core::level::db_to_amplitude(db - 0.5),
            rms_l: vibez_core::level::db_to_amplitude(db - 6.0),
            rms_r: vibez_core::level::db_to_amplitude(db - 6.5),
        };
        let closed = CardPanelState {
            input: level(-4.0),
            output: level(-6.0),
            last_meter_at: Some(std::time::Instant::now()),
            ..Default::default()
        };
        Self {
            track: TrackId::new(),
            effect,
            open: CardPanelState {
                advanced: true,
                ..closed.clone()
            },
            spectrum: CardPanelState {
                plot: crate::domains::saturator::SPECTRUM_PLOT,
                ..closed.clone()
            },
            closed,
        }
    }

    fn scene<'a>(
        &'a self,
        label: &'static str,
        card_width: f32,
        section: (f32, f32),
        panel: &'a crate::domains::device_card::CardPanelState,
    ) -> Element<'a, Message> {
        let accent = th::track_color(0);
        let effect = &self.effect;
        let track = self.track;
        let body = iced::widget::responsive(move |size| {
            crate::widgets::saturator_device::SaturatorView {
                track_id: track,
                effect,
                accent,
                panel,
                entry: None,
            }
            .view(
                crate::widgets::device_preset_bar::view(
                    track,
                    effect,
                    None,
                    accent,
                    crate::widgets::saturator_device::preset_bar_width(size),
                ),
                size,
            )
        });
        framed_card(
            label,
            card_width,
            section,
            track,
            effect,
            accent,
            body.into(),
        )
    }

    fn view(&self) -> Element<'_, Message> {
        use crate::widgets::saturator_device::{COMPACT_W, FULL_W, MEDIUM_W};
        // Sections the default 280 px and minimum 180 px detail panels
        // give a card, measured against the app.
        let (default, minimum) = (240.0, 140.0);
        let (closed, open, spectrum) = (&self.closed, &self.open, &self.spectrum);
        let wide = 1_150.0;
        container(
            column![
                row![
                    self.scene("Expanded, Advanced open", wide, (wide + 16.0, 400.0), open),
                    self.scene(
                        "Compact, tall",
                        COMPACT_W + 40.0,
                        (COMPACT_W + 56.0, 400.0),
                        closed
                    ),
                ]
                .spacing(24),
                row![
                    self.scene(
                        "Medium, tall",
                        MEDIUM_W + 60.0,
                        (MEDIUM_W + 76.0, 330.0),
                        closed
                    ),
                    self.scene(
                        "Full, default panel",
                        FULL_W,
                        (FULL_W + 16.0, default),
                        closed
                    ),
                ]
                .spacing(24),
                row![
                    self.scene(
                        "Medium, default panel, Advanced",
                        MEDIUM_W,
                        (MEDIUM_W + 16.0, default),
                        open
                    ),
                    self.scene(
                        "Compact, default panel, Spectrum",
                        460.0,
                        (476.0, default),
                        spectrum
                    ),
                    self.scene(
                        "Compact, default panel, Advanced",
                        COMPACT_W,
                        (COMPACT_W + 16.0, default),
                        open
                    ),
                ]
                .spacing(24),
                row![
                    self.scene(
                        "Full, minimum panel",
                        FULL_W,
                        (FULL_W + 16.0, minimum),
                        closed
                    ),
                    self.scene(
                        "Compact, minimum panel",
                        COMPACT_W,
                        (COMPACT_W + 16.0, minimum),
                        closed
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

pub fn run_saturator() -> iced::Result {
    iced::application(
        "Saturator device gallery",
        |_: &mut SaturatorGallery, _: Message| {},
        SaturatorGallery::view,
    )
    .theme(|_| th::vibez_theme())
    .antialiasing(true)
    .window_size((1900.0, 1500.0))
    .font(crate::icons::ICON_FONT_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_MEDIUM_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_SEMIBOLD_BYTES)
    .font(crate::typography::PLEX_MONO_MEDIUM_BYTES)
    .font(crate::typography::PLEX_MONO_SEMIBOLD_BYTES)
    .run_with(|| (SaturatorGallery::new(), iced::Task::none()))
}

/// Native Limiter at each width tier and section height it adapts to, on
/// Transparent Ceiling, with twelve seconds of drum hits pushed into the
/// ceiling standing in for live audio.
struct LimiterGallery {
    track: TrackId,
    effect: UiEffect,
    closed: crate::domains::device_card::CardPanelState,
    open: crate::domains::device_card::CardPanelState,
    spectrum: crate::domains::device_card::CardPanelState,
}

impl LimiterGallery {
    fn new() -> Self {
        use crate::domains::device_card::CardPanelState;
        let mut effect = effect(EffectType::Limiter, None, false, false, None, 0);
        effect.params = effect.descriptors.iter().map(|d| d.default).collect();
        let level = |db: f32| vibez_engine::metering::MeterValues {
            peak_l: vibez_core::level::db_to_amplitude(db),
            peak_r: vibez_core::level::db_to_amplitude(db - 0.5),
            rms_l: vibez_core::level::db_to_amplitude(db - 6.0),
            rms_r: vibez_core::level::db_to_amplitude(db - 6.5),
        };
        effect.params[vibez_dsp::limiter::param::INPUT_DB] = 3.0;
        let now = std::time::Instant::now();
        let mut dynamics = crate::domains::device_levels::ReadingHistory::default();
        for step in (0..600).rev() {
            let at = now - std::time::Duration::from_millis(step * 20);
            let beat = (step % 25) as f32;
            let hit = (-beat / 3.0).exp();
            let wobble = ((step * 37 % 11) as f32 - 5.0) * 0.8;
            dynamics.push(
                at,
                crate::domains::device_card::DynamicsReading {
                    input_db: -14.0 + 14.0 * hit + wobble,
                    reduction_db: (3.5 * hit - 0.4).max(0.0),
                },
                crate::domains::device_card::DYNAMICS_HISTORY,
            );
        }
        let closed = CardPanelState {
            input: level(-2.0),
            output: level(-1.2),
            reduction_db: 2.6,
            dynamics,
            last_meter_at: Some(now),
            ..Default::default()
        };
        Self {
            track: TrackId::new(),
            effect,
            open: CardPanelState {
                advanced: true,
                ..closed.clone()
            },
            spectrum: closed.clone(),
            closed,
        }
    }

    fn scene<'a>(
        &'a self,
        label: &'static str,
        card_width: f32,
        section: (f32, f32),
        panel: &'a crate::domains::device_card::CardPanelState,
    ) -> Element<'a, Message> {
        let accent = th::track_color(0);
        let effect = &self.effect;
        let track = self.track;
        let body = iced::widget::responsive(move |size| {
            crate::widgets::limiter_device::LimiterView {
                track_id: track,
                effect,
                accent,
                panel,
                entry: None,
                now: panel.last_meter_at.unwrap_or_else(std::time::Instant::now),
            }
            .view(
                crate::widgets::device_preset_bar::view(
                    track,
                    effect,
                    None,
                    accent,
                    crate::widgets::limiter_device::preset_bar_width(size),
                ),
                size,
            )
        });
        framed_card(
            label,
            card_width,
            section,
            track,
            effect,
            accent,
            body.into(),
        )
    }

    fn view(&self) -> Element<'_, Message> {
        use crate::widgets::limiter_device::{COMPACT_W, FULL_W, MEDIUM_W};
        // Sections the default 280 px and minimum 180 px detail panels
        // give a card, measured against the app.
        let (default, minimum) = (240.0, 140.0);
        let (closed, open, spectrum) = (&self.closed, &self.open, &self.spectrum);
        let wide = 1_150.0;
        container(
            column![
                row![
                    self.scene("Expanded, Advanced open", wide, (wide + 16.0, 400.0), open),
                    self.scene(
                        "Compact, tall",
                        COMPACT_W + 40.0,
                        (COMPACT_W + 56.0, 400.0),
                        closed
                    ),
                ]
                .spacing(24),
                row![
                    self.scene(
                        "Medium, tall",
                        MEDIUM_W + 60.0,
                        (MEDIUM_W + 76.0, 330.0),
                        closed
                    ),
                    self.scene(
                        "Full, default panel",
                        FULL_W,
                        (FULL_W + 16.0, default),
                        closed
                    ),
                ]
                .spacing(24),
                row![
                    self.scene(
                        "Medium, default panel, Advanced",
                        MEDIUM_W,
                        (MEDIUM_W + 16.0, default),
                        open
                    ),
                    self.scene("Compact, default panel", 460.0, (476.0, default), spectrum),
                    self.scene(
                        "Compact, default panel, Advanced",
                        COMPACT_W,
                        (COMPACT_W + 16.0, default),
                        open
                    ),
                ]
                .spacing(24),
                row![
                    self.scene(
                        "Compact, minimum panel",
                        COMPACT_W,
                        (COMPACT_W + 16.0, minimum),
                        closed
                    ),
                    self.scene(
                        "Compact, minimum panel, Advanced",
                        COMPACT_W,
                        (COMPACT_W + 16.0, minimum),
                        open
                    ),
                    self.scene(
                        "Medium, minimum panel, Advanced",
                        MEDIUM_W,
                        (MEDIUM_W + 16.0, minimum),
                        open
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

pub fn run_limiter() -> iced::Result {
    iced::application(
        "Limiter device gallery",
        |_: &mut LimiterGallery, _: Message| {},
        LimiterGallery::view,
    )
    .theme(|_| th::vibez_theme())
    .antialiasing(true)
    .window_size((1900.0, 1500.0))
    .font(crate::icons::ICON_FONT_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_MEDIUM_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_SEMIBOLD_BYTES)
    .font(crate::typography::PLEX_MONO_MEDIUM_BYTES)
    .font(crate::typography::PLEX_MONO_SEMIBOLD_BYTES)
    .run_with(|| (LimiterGallery::new(), iced::Task::none()))
}

/// Native Reverb at each width tier and section height it adapts to, on the
/// Warm Hall preset as in the concept, with an output level standing in for
/// live audio.
struct ReverbGallery {
    track: TrackId,
    effect: UiEffect,
    closed: crate::domains::device_card::CardPanelState,
    open: crate::domains::device_card::CardPanelState,
}

impl ReverbGallery {
    fn new() -> Self {
        use crate::domains::device_card::CardPanelState;
        use vibez_dsp::reverb::param;
        let mut effect = effect(EffectType::Reverb, None, false, false, None, 0);
        effect.params = effect.descriptors.iter().map(|d| d.default).collect();
        effect.params[param::ROOM_SIZE] = vibez_dsp::reverb::room_for_decay(2.4);
        effect.params[param::DAMPING] = 0.6;
        effect.params[param::MIX] = 0.22;
        effect.params[param::PREDELAY_MS] = 24.0;
        effect.params[param::DIFFUSION] = 0.8;
        effect.params[param::LOW_CUT_HZ] = 120.0;
        effect.params[param::HIGH_CUT_HZ] = 10_000.0;
        let level = |db: f32| vibez_engine::metering::MeterValues {
            peak_l: vibez_core::level::db_to_amplitude(db),
            peak_r: vibez_core::level::db_to_amplitude(db - 0.5),
            rms_l: vibez_core::level::db_to_amplitude(db - 6.0),
            rms_r: vibez_core::level::db_to_amplitude(db - 6.5),
        };
        let closed = CardPanelState {
            input: level(-2.0),
            output: level(-3.0),
            last_meter_at: Some(std::time::Instant::now()),
            ..Default::default()
        };
        Self {
            track: TrackId::new(),
            effect,
            open: CardPanelState {
                advanced: true,
                ..closed.clone()
            },
            closed,
        }
    }

    fn scene<'a>(
        &'a self,
        label: &'static str,
        card_width: f32,
        section: (f32, f32),
        panel: &'a crate::domains::device_card::CardPanelState,
    ) -> Element<'a, Message> {
        let accent = th::track_color(0);
        let effect = &self.effect;
        let track = self.track;
        let body = iced::widget::responsive(move |size| {
            crate::widgets::reverb_device::ReverbView {
                track_id: track,
                effect,
                accent,
                panel,
                entry: None,
                bpm: 120.0,
            }
            .view(
                crate::widgets::device_preset_bar::view(
                    track,
                    effect,
                    None,
                    accent,
                    crate::widgets::reverb_device::preset_bar_width(size),
                ),
                size,
            )
        });
        framed_card(
            label,
            card_width,
            section,
            track,
            effect,
            accent,
            body.into(),
        )
    }

    fn view(&self) -> Element<'_, Message> {
        use crate::widgets::reverb_device::COMPACT_W;
        let wide = 900.0;
        let (closed, open) = (&self.closed, &self.open);
        container(
            column![
                row![
                    self.scene("Wide, tall", wide, (wide + 16.0, 330.0), closed),
                    self.scene(
                        "Compact, tall",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 330.0),
                        closed
                    ),
                ]
                .spacing(24),
                row![
                    self.scene("Wide, Advanced open", wide, (wide + 16.0, 400.0), open),
                    self.scene(
                        "Compact, Advanced open",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 400.0),
                        open
                    ),
                ]
                .spacing(24),
                row![
                    self.scene("Wide, shortest", wide, (wide + 16.0, 110.0), closed),
                    self.scene("Compact, short", COMPACT_W, (COMPACT_W + 16.0, 150.0), open),
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

pub fn run_reverb() -> iced::Result {
    iced::application(
        "Reverb device gallery",
        |_: &mut ReverbGallery, _: Message| {},
        ReverbGallery::view,
    )
    .theme(|_| th::vibez_theme())
    .antialiasing(true)
    .window_size((1900.0, 1260.0))
    .font(crate::icons::ICON_FONT_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_MEDIUM_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_SEMIBOLD_BYTES)
    .font(crate::typography::PLEX_MONO_MEDIUM_BYTES)
    .font(crate::typography::PLEX_MONO_SEMIBOLD_BYTES)
    .run_with(|| (ReverbGallery::new(), iced::Task::none()))
}

/// Native Auto-Pan at each width tier and section height it adapts to, on
/// the Wide Eighths preset as in the concept, with an output a little to
/// the right standing in for live audio.
struct AutoPanGallery {
    track: TrackId,
    effect: UiEffect,
    closed: crate::domains::device_card::CardPanelState,
    open: crate::domains::device_card::CardPanelState,
}

impl AutoPanGallery {
    fn new() -> Self {
        use crate::domains::device_card::CardPanelState;
        use vibez_dsp::auto_pan::param;
        let mut effect = effect(EffectType::AutoPan, None, false, false, None, 0);
        effect.params = effect.descriptors.iter().map(|d| d.default).collect();
        effect.params[param::SYNC] = 5.0;
        effect.params[param::DEPTH] = 0.75;
        effect.params[param::SMOOTHING] = 0.05;
        let level = |db: f32| vibez_engine::metering::MeterValues {
            peak_l: vibez_core::level::db_to_amplitude(db),
            peak_r: vibez_core::level::db_to_amplitude(db - 0.5),
            rms_l: vibez_core::level::db_to_amplitude(db - 6.0),
            rms_r: vibez_core::level::db_to_amplitude(db - 6.5),
        };
        let closed = CardPanelState {
            input: level(-3.0),
            output: vibez_engine::metering::MeterValues {
                rms_l: vibez_core::level::db_to_amplitude(-12.0),
                ..level(-3.0)
            },
            last_meter_at: Some(std::time::Instant::now()),
            ..Default::default()
        };
        Self {
            track: TrackId::new(),
            effect,
            open: CardPanelState {
                advanced: true,
                ..closed.clone()
            },
            closed,
        }
    }

    fn scene<'a>(
        &'a self,
        label: &'static str,
        card_width: f32,
        section: (f32, f32),
        panel: &'a crate::domains::device_card::CardPanelState,
    ) -> Element<'a, Message> {
        let accent = th::track_color(0);
        let effect = &self.effect;
        let track = self.track;
        let body = iced::widget::responsive(move |size| {
            crate::widgets::auto_pan_device::AutoPanView {
                track_id: track,
                effect,
                accent,
                panel,
                entry: None,
                bpm: 120.0,
            }
            .view(
                crate::widgets::device_preset_bar::view(
                    track,
                    effect,
                    None,
                    accent,
                    crate::widgets::auto_pan_device::preset_bar_width(size),
                ),
                size,
            )
        });
        framed_card(
            label,
            card_width,
            section,
            track,
            effect,
            accent,
            body.into(),
        )
    }

    fn view(&self) -> Element<'_, Message> {
        use crate::widgets::auto_pan_device::COMPACT_W;
        let wide = 900.0;
        let (closed, open) = (&self.closed, &self.open);
        container(
            column![
                row![
                    self.scene("Wide, tall", wide, (wide + 16.0, 330.0), closed),
                    self.scene(
                        "Compact, tall",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 330.0),
                        closed
                    ),
                ]
                .spacing(24),
                row![
                    self.scene("Wide, Advanced open", wide, (wide + 16.0, 400.0), open),
                    self.scene(
                        "Compact, Advanced open",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 400.0),
                        open
                    ),
                ]
                .spacing(24),
                row![
                    self.scene("Wide, shortest", wide, (wide + 16.0, 110.0), closed),
                    self.scene("Compact, short", COMPACT_W, (COMPACT_W + 16.0, 150.0), open),
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

pub fn run_auto_pan() -> iced::Result {
    iced::application(
        "Auto-Pan device gallery",
        |_: &mut AutoPanGallery, _: Message| {},
        AutoPanGallery::view,
    )
    .theme(|_| th::vibez_theme())
    .antialiasing(true)
    .window_size((1900.0, 1260.0))
    .font(crate::icons::ICON_FONT_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_MEDIUM_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_SEMIBOLD_BYTES)
    .font(crate::typography::PLEX_MONO_MEDIUM_BYTES)
    .font(crate::typography::PLEX_MONO_SEMIBOLD_BYTES)
    .run_with(|| (AutoPanGallery::new(), iced::Task::none()))
}

/// Native Phaser at each width tier and section height it adapts to, on the
/// Slow Sweep preset as in the concept.
struct PhaserGallery {
    track: TrackId,
    effect: UiEffect,
    closed: crate::domains::device_card::CardPanelState,
    open: crate::domains::device_card::CardPanelState,
}

impl PhaserGallery {
    fn new() -> Self {
        use crate::domains::device_card::CardPanelState;
        use vibez_dsp::phaser::param;
        let mut effect = effect(EffectType::Phaser, None, false, false, None, 0);
        effect.params = effect.descriptors.iter().map(|d| d.default).collect();
        effect.params[param::RATE] = 0.5;
        effect.params[param::DEPTH] = 0.6;
        effect.params[param::FEEDBACK] = 0.3;
        effect.params[param::MIX] = 0.5;
        effect.params[param::STEREO_PHASE] = 90.0;
        let closed = CardPanelState::default();
        Self {
            track: TrackId::new(),
            effect,
            open: CardPanelState {
                advanced: true,
                ..closed.clone()
            },
            closed,
        }
    }

    fn scene<'a>(
        &'a self,
        label: &'static str,
        card_width: f32,
        section: (f32, f32),
        panel: &'a crate::domains::device_card::CardPanelState,
    ) -> Element<'a, Message> {
        let accent = th::track_color(0);
        let effect = &self.effect;
        let track = self.track;
        let body = iced::widget::responsive(move |size| {
            crate::widgets::phaser_device::PhaserView {
                track_id: track,
                effect,
                accent,
                panel,
                entry: None,
                bpm: 120.0,
            }
            .view(
                crate::widgets::device_preset_bar::view(
                    track,
                    effect,
                    None,
                    accent,
                    crate::widgets::phaser_device::preset_bar_width(size),
                ),
                size,
            )
        });
        framed_card(
            label,
            card_width,
            section,
            track,
            effect,
            accent,
            body.into(),
        )
    }

    fn view(&self) -> Element<'_, Message> {
        use crate::widgets::phaser_device::COMPACT_W;
        let wide = 900.0;
        let (closed, open) = (&self.closed, &self.open);
        container(
            column![
                row![
                    self.scene("Wide, tall", wide, (wide + 16.0, 330.0), closed),
                    self.scene(
                        "Compact, tall",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 330.0),
                        closed
                    ),
                ]
                .spacing(24),
                row![
                    self.scene("Wide, Advanced open", wide, (wide + 16.0, 400.0), open),
                    self.scene(
                        "Compact, Advanced open",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 400.0),
                        open
                    ),
                ]
                .spacing(24),
                row![
                    self.scene("Wide, shortest", wide, (wide + 16.0, 110.0), closed),
                    self.scene("Compact, short", COMPACT_W, (COMPACT_W + 16.0, 150.0), open),
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

pub fn run_phaser() -> iced::Result {
    iced::application(
        "Phaser device gallery",
        |_: &mut PhaserGallery, _: Message| {},
        PhaserGallery::view,
    )
    .theme(|_| th::vibez_theme())
    .antialiasing(true)
    .window_size((1900.0, 1260.0))
    .font(crate::icons::ICON_FONT_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_MEDIUM_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_SEMIBOLD_BYTES)
    .font(crate::typography::PLEX_MONO_MEDIUM_BYTES)
    .font(crate::typography::PLEX_MONO_SEMIBOLD_BYTES)
    .run_with(|| (PhaserGallery::new(), iced::Task::none()))
}

/// Native Subtractive Synth at each width tier, tab and height it adapts
/// to, on the Soft Analog sound from the concept.
struct SynthGallery {
    track: ProjectTrack,
}

impl SynthGallery {
    fn new() -> Self {
        use crate::domains::device_params::DeviceParams;
        use vibez_core::device_preset::DevicePresetKind;
        use vibez_core::midi::InstrumentKind;
        let mut track = ProjectTrack::new(TrackId::new(), "Keys".into(), 0);
        track.kind = vibez_core::midi::TrackKind::Midi;
        track.has_instrument = true;
        track.instrument_kind = Some(InstrumentKind::SubtractiveSynth);
        track.instrument_params = vibez_instruments::synth::SYNTH_PARAMS
            .iter()
            .map(|d| d.default)
            .collect();
        let soft_analog = crate::domains::device_presets::presets_for(
            DevicePresetKind::Instrument(InstrumentKind::SubtractiveSynth),
        )
        .into_iter()
        .find(|preset| preset.name() == "Soft Analog");
        let updates = soft_analog.and_then(|preset| {
            crate::domains::device_presets::prepare_updates(
                DeviceParams::instrument(&track)?,
                &preset,
            )
            .ok()
        });
        for (index, value) in updates.unwrap_or_default() {
            track.instrument_params[index] = value;
        }
        Self { track }
    }

    fn scene<'a>(
        &'a self,
        label: &'static str,
        card_width: f32,
        section: (f32, f32),
        panel: crate::domains::instrument_card::InstrumentPanel,
    ) -> Element<'a, Message> {
        let accent = th::track_color(0);
        let track_id = self.track.id;
        let Some(device) = crate::domains::device_params::DeviceParams::instrument(&self.track)
        else {
            return text("no synth").into();
        };
        let body = iced::widget::responsive(move |size| {
            crate::widgets::synth_device::SynthView {
                track_id,
                device,
                accent,
                panel: panel.clone(),
                entry: None,
            }
            .view(
                crate::widgets::device_preset_bar::view(
                    track_id,
                    device,
                    None,
                    accent,
                    crate::widgets::synth_device::preset_bar_width(size),
                ),
                size,
            )
        });
        let title = crate::widgets::device_chrome::title_bar(
            row![
                text("\u{25CF}").size(8).color(accent),
                text("Subtractive Synth").size(11).color(th::text())
            ]
            .spacing(5)
            .align_y(iced::Alignment::Center),
        );
        let card = crate::widgets::device_chrome::card(
            column![title, body].width(Length::Fixed(card_width)),
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
        use crate::domains::instrument_card::InstrumentPanel;
        use crate::domains::synth::SynthPage;
        use crate::widgets::synth_device::{COMPACT_W, FULL_W};
        let full = FULL_W + 40.0;
        let medium = 760.0;
        let page = |page: SynthPage| InstrumentPanel {
            page: page.index(),
            ..Default::default()
        };
        container(
            column![
                self.scene(
                    "Full, tall",
                    full,
                    (full + 16.0, 330.0),
                    page(SynthPage::Oscillator)
                ),
                row![
                    self.scene(
                        "Medium, Filter tab",
                        medium,
                        (medium + 16.0, 330.0),
                        page(SynthPage::Filter)
                    ),
                    self.scene(
                        "Compact, Oscillator tab",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 330.0),
                        page(SynthPage::Oscillator)
                    ),
                ]
                .spacing(24),
                row![
                    self.scene(
                        "Medium, Modulation tab, short",
                        medium,
                        (medium + 16.0, 210.0),
                        page(SynthPage::Modulation)
                    ),
                    self.scene(
                        "Compact, Envelope tab, filter",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 330.0),
                        InstrumentPanel {
                            page: SynthPage::Envelope.index(),
                            second_envelope: true,
                            ..Default::default()
                        }
                    ),
                ]
                .spacing(24),
                self.scene(
                    "Full, shortest",
                    full,
                    (full + 16.0, 110.0),
                    page(SynthPage::Oscillator)
                ),
            ]
            .spacing(14),
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

pub fn run_synth() -> iced::Result {
    iced::application(
        "Subtractive Synth device gallery",
        |_: &mut SynthGallery, _: Message| {},
        SynthGallery::view,
    )
    .theme(|_| th::vibez_theme())
    .antialiasing(true)
    .window_size((1900.0, 1400.0))
    .font(crate::icons::ICON_FONT_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_MEDIUM_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_SEMIBOLD_BYTES)
    .font(crate::typography::PLEX_MONO_MEDIUM_BYTES)
    .font(crate::typography::PLEX_MONO_SEMIBOLD_BYTES)
    .run_with(|| (SynthGallery::new(), iced::Task::none()))
}

/// A tabbed instrument card titled `name` in a section `section` big,
/// under `label`: `body` builds the card's body from the track's controls,
/// a preset bar and the body's size.
fn tabbed_scene<'a>(
    label: &'static str,
    name: &'static str,
    track: &'a ProjectTrack,
    card_width: f32,
    section: (f32, f32),
    body: impl Fn(
            crate::domains::device_params::DeviceParams<'a>,
            Element<'a, Message>,
            iced::Size,
        ) -> Element<'a, Message>
        + 'a,
) -> Element<'a, Message> {
    let accent = th::track_color(0);
    let track_id = track.id;
    let Some(device) = crate::domains::device_params::DeviceParams::instrument(track) else {
        return text("no instrument").into();
    };
    let body = iced::widget::responsive(move |size| {
        body(
            device,
            crate::widgets::device_preset_bar::view(
                track_id,
                device,
                None,
                accent,
                crate::widgets::instrument_bands::preset_bar_width(size),
            ),
            size,
        )
    });
    let title = crate::widgets::device_chrome::title_bar(
        row![
            text("\u{25CF}").size(8).color(accent),
            text(name).size(11).color(th::text())
        ]
        .spacing(5)
        .align_y(iced::Alignment::Center),
    );
    let card =
        crate::widgets::device_chrome::card(column![title, body].width(Length::Fixed(card_width)));
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

/// Native Wavetable Synth at each width tier, tab and height it adapts to,
/// on the concept's Glass Motion, with scenes at the bodies the default and
/// minimum detail panels give an instrument card.
struct WavetableGallery {
    track: ProjectTrack,
}

impl WavetableGallery {
    fn new() -> Self {
        use crate::domains::device_params::DeviceParams;
        use vibez_core::device_preset::DevicePresetKind;
        use vibez_core::midi::InstrumentKind;
        let mut track = ProjectTrack::new(TrackId::new(), "Pads".into(), 0);
        track.kind = vibez_core::midi::TrackKind::Midi;
        track.has_instrument = true;
        track.instrument_kind = Some(InstrumentKind::WavetableSynth);
        track.instrument_params = vibez_instruments::wavetable::WAVETABLE_PARAMS
            .iter()
            .map(|d| d.default)
            .collect();
        let glass_motion = crate::domains::device_presets::presets_for(
            DevicePresetKind::Instrument(InstrumentKind::WavetableSynth),
        )
        .into_iter()
        .find(|preset| preset.name() == "Glass Motion");
        let updates = glass_motion.and_then(|preset| {
            crate::domains::device_presets::prepare_updates(
                DeviceParams::instrument(&track)?,
                &preset,
            )
            .ok()
        });
        for (index, value) in updates.unwrap_or_default() {
            track.instrument_params[index] = value;
        }
        Self { track }
    }

    fn scene<'a>(
        &'a self,
        label: &'static str,
        card_width: f32,
        section: (f32, f32),
        panel: crate::domains::instrument_card::InstrumentPanel,
    ) -> Element<'a, Message> {
        let accent = th::track_color(0);
        let track_id = self.track.id;
        tabbed_scene(
            label,
            "Wavetable Synth",
            &self.track,
            card_width,
            section,
            move |device, preset_bar, size| {
                crate::widgets::wavetable_device::WavetableView {
                    track_id,
                    device,
                    accent,
                    panel: panel.clone(),
                    entry: None,
                }
                .view(preset_bar, size)
            },
        )
    }

    fn view(&self) -> Element<'_, Message> {
        use crate::domains::instrument_card::InstrumentPanel;
        use crate::domains::wavetable::WavetablePage;
        use crate::widgets::wavetable_device::{COMPACT_W, FULL_W};
        let full = FULL_W + 40.0;
        let medium = 760.0;
        // Sections that give a card the bodies the default 280 px and
        // minimum 180 px detail panels do.
        let (default, minimum) = (210.0, 110.0);
        let page = |page: WavetablePage| InstrumentPanel {
            page: page.index(),
            ..Default::default()
        };
        container(
            column![
                row![
                    self.scene(
                        "Full, tall (overview)",
                        full,
                        (full + 16.0, 430.0),
                        page(WavetablePage::Oscillators)
                    ),
                    self.scene(
                        "Compact, Filter tab",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 330.0),
                        page(WavetablePage::Filter)
                    ),
                ]
                .spacing(24),
                row![
                    self.scene(
                        "Medium, Oscillators tab",
                        medium,
                        (medium + 16.0, 330.0),
                        page(WavetablePage::Oscillators)
                    ),
                    self.scene(
                        "Medium, Envelopes tab, default panel",
                        medium,
                        (medium + 16.0, default),
                        page(WavetablePage::Envelopes)
                    ),
                ]
                .spacing(24),
                row![
                    self.scene(
                        "Full, Oscillators tab, default panel",
                        full,
                        (full + 16.0, default),
                        page(WavetablePage::Oscillators)
                    ),
                    self.scene(
                        "Compact, Osc B, default panel",
                        COMPACT_W,
                        (COMPACT_W + 16.0, default),
                        InstrumentPanel {
                            oscillator: 1,
                            ..Default::default()
                        }
                    ),
                ]
                .spacing(24),
                row![
                    self.scene(
                        "Compact, Modulation tab, default panel",
                        COMPACT_W,
                        (COMPACT_W + 16.0, default),
                        page(WavetablePage::Modulation)
                    ),
                    self.scene(
                        "Full, minimum panel",
                        full,
                        (full + 16.0, minimum),
                        page(WavetablePage::Oscillators)
                    ),
                ]
                .spacing(24),
                self.scene(
                    "Compact, Envelopes tab, minimum panel",
                    COMPACT_W,
                    (COMPACT_W + 16.0, minimum),
                    page(WavetablePage::Envelopes)
                ),
            ]
            .spacing(14),
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

pub fn run_wavetable() -> iced::Result {
    iced::application(
        "Wavetable Synth device gallery",
        |_: &mut WavetableGallery, _: Message| {},
        WavetableGallery::view,
    )
    .theme(|_| th::vibez_theme())
    .antialiasing(true)
    .window_size((1900.0, 1500.0))
    .font(crate::icons::ICON_FONT_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_MEDIUM_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_SEMIBOLD_BYTES)
    .font(crate::typography::PLEX_MONO_MEDIUM_BYTES)
    .font(crate::typography::PLEX_MONO_SEMIBOLD_BYTES)
    .run_with(|| (WavetableGallery::new(), iced::Task::none()))
}

/// Native Sampler at each width tier, tab and height it adapts to, looping
/// a generated electric-piano tone.
struct SamplerGallery {
    track: ProjectTrack,
}

impl SamplerGallery {
    fn new() -> Self {
        use vibez_instruments::sampler::{filter_type, param, SAMPLER_PARAMS};
        let rate = 44_100.0f32;
        let tone: Vec<f32> = (0..(rate * 8.4) as usize)
            .map(|i| {
                let t = i as f32 / rate;
                let mut s = 0.0;
                for (k, a) in [(1.0, 1.0), (2.0, 0.5), (3.0, 0.25), (4.0, 0.12)] {
                    s += a
                        * (std::f32::consts::TAU * 261.63 * k * t).sin()
                        * (-t * (0.4 + k * 0.3)).exp();
                }
                s * 0.5 * (1.0 + 0.3 * (t * 37.0).sin() * (t * 3.1).sin())
            })
            .collect();
        let mut track = ProjectTrack::new(TrackId::new(), "Keys".into(), 0);
        track.kind = vibez_core::midi::TrackKind::Midi;
        track.has_instrument = true;
        track.instrument_kind = Some(vibez_core::midi::InstrumentKind::Sampler);
        track.instrument_params = SAMPLER_PARAMS.iter().map(|d| d.default).collect();
        for (index, value) in [
            (param::START, 0.06),
            (param::END, 0.9),
            (param::LOOP, 1.0),
            (param::LOOP_START, 0.4),
            (param::LOOP_END, 0.68),
            (param::CROSSFADE, 12.0),
            (param::VOLUME, 0.7),
            (param::ATTACK, 0.012),
            (param::DECAY, 0.18),
            (param::SUSTAIN, 0.5),
            (param::RELEASE, 1.2),
            (param::FILTER_TYPE, filter_type::LP24 as f32),
            (param::CUTOFF, 3_200.0),
            (param::FILTER_ENV, 1.5),
        ] {
            track.instrument_params[index] = value;
        }
        track.sample_name = Some("Rhodes_C3.wav".into());
        track.sample_audio = Some(std::sync::Arc::new(
            vibez_core::audio_buffer::DecodedAudio {
                channels: vec![tone.clone(), tone],
                sample_rate: rate as u32,
            },
        ));
        Self { track }
    }

    fn scene<'a>(
        &'a self,
        label: &'static str,
        card_width: f32,
        section: (f32, f32),
        panel: crate::domains::instrument_card::InstrumentPanel,
    ) -> Element<'a, Message> {
        let accent = th::track_color(0);
        let track_id = self.track.id;
        let track = &self.track;
        let Some(device) = crate::domains::device_params::DeviceParams::instrument(track) else {
            return text("no sampler").into();
        };
        let body = iced::widget::responsive(move |size| {
            crate::widgets::sampler_device::SamplerView {
                track_id,
                device,
                load: Message::LoadSamplerSample(track_id),
                audio: track.sample_audio.as_ref(),
                sample_name: track.sample_name.clone(),
                accent,
                panel: panel.clone(),
                entry: None,
            }
            .view(
                crate::widgets::device_preset_bar::view(
                    track_id,
                    device,
                    None,
                    accent,
                    crate::widgets::sampler_device::preset_bar_width(size),
                ),
                size,
            )
        });
        let title = crate::widgets::device_chrome::title_bar(
            row![
                text("\u{25CF}").size(8).color(accent),
                text("Sampler").size(11).color(th::text())
            ]
            .spacing(5)
            .align_y(iced::Alignment::Center),
        );
        let card = crate::widgets::device_chrome::card(
            column![title, body].width(Length::Fixed(card_width)),
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
        use crate::domains::instrument_card::InstrumentPanel;
        use crate::domains::sampler::SamplerPage;
        use crate::widgets::sampler_device::COMPACT_W;
        let full = 1_100.0;
        let medium = 760.0;
        let page = |page: SamplerPage, second_envelope: bool| InstrumentPanel {
            page: page.index(),
            second_envelope,
            ..Default::default()
        };
        container(
            column![
                self.scene(
                    "Full, tall",
                    full,
                    (full + 16.0, 360.0),
                    page(SamplerPage::Sample, false)
                ),
                row![
                    self.scene(
                        "Medium, Filter tab",
                        medium,
                        (medium + 16.0, 360.0),
                        page(SamplerPage::Filter, false)
                    ),
                    self.scene(
                        "Compact, Sample tab",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 360.0),
                        page(SamplerPage::Sample, false)
                    ),
                ]
                .spacing(24),
                row![
                    self.scene(
                        "Medium, Zones tab, short",
                        medium,
                        (medium + 16.0, 250.0),
                        page(SamplerPage::Zones, false)
                    ),
                    self.scene(
                        "Compact, Filter tab, envelope",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 300.0),
                        page(SamplerPage::Filter, true)
                    ),
                ]
                .spacing(24),
                self.scene(
                    "Full, shortest",
                    full,
                    (full + 16.0, 120.0),
                    page(SamplerPage::Sample, false)
                ),
            ]
            .spacing(14),
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

pub fn run_sampler() -> iced::Result {
    iced::application(
        "Sampler device gallery",
        |_: &mut SamplerGallery, _: Message| {},
        SamplerGallery::view,
    )
    .theme(|_| th::vibez_theme())
    .antialiasing(true)
    .window_size((1900.0, 1500.0))
    .font(crate::icons::ICON_FONT_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_MEDIUM_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_SEMIBOLD_BYTES)
    .font(crate::typography::PLEX_MONO_MEDIUM_BYTES)
    .font(crate::typography::PLEX_MONO_SEMIBOLD_BYTES)
    .run_with(|| (SamplerGallery::new(), iced::Task::none()))
}

/// Native Drum Rack at each width and height it adapts to, with a kit of
/// generated hits in its first bank.
struct DrumRackGallery {
    track: ProjectTrack,
}

impl DrumRackGallery {
    fn new() -> Self {
        use crate::state::UiDrumPad;
        use vibez_instruments::sampler::param;
        let rate = 44_100.0f32;
        let mut seed = 0x9e37_79b9_u32;
        let mut noise = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as f32 / u32::MAX as f32 * 2.0 - 1.0
        };
        let mut hit = |seconds: f32, tone: f32, sweep: f32, grit: f32, decay: f32| {
            let samples: Vec<f32> = (0..(rate * seconds) as usize)
                .map(|i| {
                    let t = i as f32 / rate;
                    let pitch = tone * (1.0 + sweep * (-t * 30.0).exp());
                    let body = (std::f32::consts::TAU * pitch * t).sin() * (1.0 - grit);
                    (body + noise() * grit) * (-t * decay).exp() * 0.9
                })
                .collect();
            std::sync::Arc::new(vibez_core::audio_buffer::DecodedAudio {
                channels: vec![samples.clone(), samples],
                sample_rate: rate as u32,
            })
        };
        let kit = [
            ("Kick", hit(0.6, 50.0, 3.0, 0.05, 7.0)),
            ("Snare", hit(0.35, 190.0, 0.5, 0.6, 14.0)),
            ("Closed Hat", hit(0.12, 0.0, 0.0, 1.0, 40.0)),
            ("Open Hat Long Tail 03", hit(0.8, 0.0, 0.0, 1.0, 5.0)),
            ("Clap", hit(0.3, 0.0, 0.0, 0.9, 16.0)),
            ("Rim", hit(0.08, 800.0, 0.2, 0.3, 50.0)),
            ("Tom Low", hit(0.5, 90.0, 0.8, 0.1, 8.0)),
            ("Tom High", hit(0.4, 150.0, 0.8, 0.1, 9.0)),
            ("Shaker", hit(0.2, 0.0, 0.0, 1.0, 22.0)),
            ("Ride", hit(1.2, 520.0, 0.0, 0.7, 3.0)),
            ("Crash", hit(1.6, 0.0, 0.0, 1.0, 2.0)),
        ];
        let mut track = ProjectTrack::new(TrackId::new(), "Drums".into(), 0);
        track.kind = vibez_core::midi::TrackKind::Midi;
        track.has_instrument = true;
        track.instrument_kind = Some(vibez_core::midi::InstrumentKind::DrumRack);
        track.drum_rack_pads = crate::state::default_drum_rack_pads();
        for (pad, (name, audio)) in kit.into_iter().enumerate() {
            let index = if pad < 11 { pad } else { pad + 1 };
            track.drum_rack_pads[index] = UiDrumPad {
                name: Some(name.into()),
                source: Some(vibez_core::track::MediaSourceRef::LocalFile {
                    path: format!("/kits/dusty/{}.wav", name.to_lowercase().replace(' ', "_"))
                        .into(),
                }),
                audio: Some(audio),
                choke_group: matches!(pad, 2 | 3).then_some(1),
                ..UiDrumPad::default()
            };
        }
        let snare = track.drum_rack_pads[1].sampler_params_mut();
        snare[param::START] = 0.02;
        snare[param::END] = 0.85;
        snare[param::TUNE] = -2.0;
        snare[param::PAN] = -0.2;
        track.drum_rack_pads[17] = UiDrumPad {
            name: Some("Perc".into()),
            audio: track.drum_rack_pads[5].audio.clone(),
            ..UiDrumPad::default()
        };
        track.selected_drum_pad = 1;
        Self { track }
    }

    fn scene<'a>(
        &'a self,
        label: &'static str,
        card_width: f32,
        section: (f32, f32),
        panel: crate::domains::instrument_card::InstrumentPanel,
    ) -> Element<'a, Message> {
        use crate::domains::device_params::{DeviceParams, DeviceSlot};
        let accent = th::track_color(2);
        let track_id = self.track.id;
        let track = &self.track;
        let selected = track.selected_drum_pad;
        let Some(device) = DeviceParams::find(track, DeviceSlot::DrumPad(selected)) else {
            return text("no pad").into();
        };
        let body = iced::widget::responsive(move |size| {
            let layout = crate::widgets::drum_rack_device::arrange(size);
            let mut firing = [false; vibez_core::track::DRUM_RACK_BANK_SIZE];
            firing[0] = true;
            crate::widgets::drum_rack_device::DrumRackView {
                track_id,
                pads: &track.drum_rack_pads,
                selected,
                device,
                firing,
                banks_firing: [true, false, false, false],
                drop_target: Some(14),
                accent,
                panel: panel.clone(),
                entry: None,
            }
            .view(
                crate::widgets::device_preset_bar::view(
                    track_id,
                    device,
                    None,
                    accent,
                    crate::widgets::drum_rack_device::preset_bar_width(layout),
                ),
                size,
            )
        });
        let title = crate::widgets::device_chrome::title_bar(
            row![
                text("\u{25CF}").size(8).color(accent),
                text("Drum Rack").size(11).color(th::text())
            ]
            .spacing(5)
            .align_y(iced::Alignment::Center),
        );
        let card = crate::widgets::device_chrome::card(
            column![title, body].width(Length::Fixed(card_width)),
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
        use crate::domains::instrument_card::InstrumentPanel;
        use crate::domains::sampler::SamplerPage;
        use crate::widgets::drum_rack_device::{COMPACT_W, SIDE_BY_SIDE_W};
        let full = 1_200.0;
        let page = |page: SamplerPage, pad_sampler: bool| InstrumentPanel {
            page: page.index(),
            pad_sampler,
            ..Default::default()
        };
        container(
            column![
                self.scene(
                    "Full, tall",
                    full,
                    (full + 16.0, 380.0),
                    page(SamplerPage::Sample, false)
                ),
                row![
                    self.scene(
                        "Side by side at its narrowest, Pitch tab",
                        SIDE_BY_SIDE_W,
                        (SIDE_BY_SIDE_W + 16.0, 330.0),
                        page(SamplerPage::Pitch, false)
                    ),
                    self.scene(
                        "Compact, Pads",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 330.0),
                        page(SamplerPage::Sample, false)
                    ),
                    self.scene(
                        "Compact, Sampler",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 330.0),
                        page(SamplerPage::Filter, true)
                    ),
                ]
                .spacing(24),
                row![
                    self.scene(
                        "Full, short",
                        full,
                        (full + 16.0, 220.0),
                        page(SamplerPage::Sample, false)
                    ),
                    self.scene(
                        "Compact, Pads, short",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 220.0),
                        page(SamplerPage::Sample, false)
                    ),
                ]
                .spacing(24),
                // The section the 180 px minimum detail panel gives a card.
                row![
                    self.scene(
                        "Full, minimum panel",
                        full,
                        (full + 16.0, 110.0),
                        page(SamplerPage::Sample, false)
                    ),
                    self.scene(
                        "Compact, minimum panel, Sampler",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 110.0),
                        page(SamplerPage::Sample, true)
                    ),
                ]
                .spacing(24),
            ]
            .spacing(14),
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

pub fn run_drum_rack() -> iced::Result {
    iced::application(
        "Drum Rack device gallery",
        |_: &mut DrumRackGallery, _: Message| {},
        DrumRackGallery::view,
    )
    .theme(|_| th::vibez_theme())
    .antialiasing(true)
    .window_size((1900.0, 1500.0))
    .subscription(|_| {
        iced::time::every(std::time::Duration::from_millis(16)).map(|_| Message::Tick)
    })
    .font(crate::icons::ICON_FONT_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_MEDIUM_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_SEMIBOLD_BYTES)
    .font(crate::typography::PLEX_MONO_MEDIUM_BYTES)
    .font(crate::typography::PLEX_MONO_SEMIBOLD_BYTES)
    .run_with(|| (DrumRackGallery::new(), iced::Task::none()))
}

/// Native FM Synth at each width tier, tab and height it adapts to, on the
/// concept's Bell Keys with Operator B shown, with scenes at the bodies the
/// default and minimum detail panels give an instrument card.
struct FmGallery {
    track: ProjectTrack,
}

impl FmGallery {
    fn new() -> Self {
        use crate::domains::device_params::DeviceParams;
        use vibez_core::device_preset::DevicePresetKind;
        use vibez_core::midi::InstrumentKind;
        let mut track = ProjectTrack::new(TrackId::new(), "Keys".into(), 0);
        track.kind = vibez_core::midi::TrackKind::Midi;
        track.has_instrument = true;
        track.instrument_kind = Some(InstrumentKind::FmSynth);
        track.instrument_params = vibez_instruments::fm::FM_PARAMS
            .iter()
            .map(|d| d.default)
            .collect();
        let bell_keys = crate::domains::device_presets::presets_for(DevicePresetKind::Instrument(
            InstrumentKind::FmSynth,
        ))
        .into_iter()
        .find(|preset| preset.name() == "Bell Keys");
        let updates = bell_keys.and_then(|preset| {
            crate::domains::device_presets::prepare_updates(
                DeviceParams::instrument(&track)?,
                &preset,
            )
            .ok()
        });
        for (index, value) in updates.unwrap_or_default() {
            track.instrument_params[index] = value;
        }
        Self { track }
    }

    fn scene<'a>(
        &'a self,
        label: &'static str,
        card_width: f32,
        section: (f32, f32),
        page: crate::domains::fm::FmPage,
    ) -> Element<'a, Message> {
        let accent = th::track_color(0);
        let track_id = self.track.id;
        let panel = crate::domains::instrument_card::InstrumentPanel {
            page: page.index(),
            oscillator: 1,
            ..Default::default()
        };
        tabbed_scene(
            label,
            "FM Synth",
            &self.track,
            card_width,
            section,
            move |device, preset_bar, size| {
                crate::widgets::fm_device::FmView {
                    track_id,
                    device,
                    accent,
                    panel: panel.clone(),
                    entry: None,
                }
                .view(preset_bar, size)
            },
        )
    }

    fn view(&self) -> Element<'_, Message> {
        use crate::domains::fm::FmPage;
        use crate::widgets::fm_device::{COMPACT_W, FULL_W};
        let full = FULL_W + 40.0;
        let medium = 760.0;
        // Sections that give a card the bodies the default 280 px and
        // minimum 180 px detail panels do.
        let (default, minimum) = (210.0, 110.0);
        container(
            column![
                row![
                    self.scene(
                        "Full, tall (overview)",
                        full,
                        (full + 16.0, 430.0),
                        FmPage::Overview
                    ),
                    self.scene(
                        "Compact, Operators tab",
                        COMPACT_W,
                        (COMPACT_W + 16.0, 330.0),
                        FmPage::Operators
                    ),
                ]
                .spacing(24),
                row![
                    self.scene(
                        "Full, Overview tab, default panel",
                        full,
                        (full + 16.0, default),
                        FmPage::Overview
                    ),
                    self.scene(
                        "Compact, Overview tab, default panel",
                        COMPACT_W,
                        (COMPACT_W + 16.0, default),
                        FmPage::Overview
                    ),
                ]
                .spacing(24),
                row![
                    self.scene(
                        "Medium, Operators tab, default panel",
                        medium,
                        (medium + 16.0, default),
                        FmPage::Operators
                    ),
                    self.scene(
                        "Medium, Envelopes tab, default panel",
                        medium,
                        (medium + 16.0, default),
                        FmPage::Envelopes
                    ),
                ]
                .spacing(24),
                row![
                    self.scene(
                        "Compact, Modulation tab, default panel",
                        COMPACT_W,
                        (COMPACT_W + 16.0, default),
                        FmPage::Modulation
                    ),
                    self.scene(
                        "Full, minimum panel",
                        full,
                        (full + 16.0, minimum),
                        FmPage::Overview
                    ),
                ]
                .spacing(24),
                self.scene(
                    "Compact, Envelopes tab, minimum panel",
                    COMPACT_W,
                    (COMPACT_W + 16.0, minimum),
                    FmPage::Envelopes
                ),
            ]
            .spacing(14),
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

pub fn run_fm() -> iced::Result {
    iced::application(
        "FM Synth device gallery",
        |_: &mut FmGallery, _: Message| {},
        FmGallery::view,
    )
    .theme(|_| th::vibez_theme())
    .antialiasing(true)
    .window_size((1900.0, 1500.0))
    .font(crate::icons::ICON_FONT_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_MEDIUM_BYTES)
    .font(crate::typography::PLEX_SANS_CONDENSED_SEMIBOLD_BYTES)
    .font(crate::typography::PLEX_MONO_MEDIUM_BYTES)
    .font(crate::typography::PLEX_MONO_SEMIBOLD_BYTES)
    .run_with(|| (FmGallery::new(), iced::Task::none()))
}
