//! Composes Gate history, native signal meters and persistent controls from measured bounds.

use crate::{
    domains::{
        device_controls::DeviceControlState,
        devices::DevicesState,
        gate::{GateMsg, GatePanel},
        gate_layout::{self, ASIDE_WIDTH, INLINE_CORE_WIDTH, OUTPUT_WIDTH, SOURCE_WIDTH},
    },
    message::Message,
    state::{ProjectTrack, UiEffect},
    theme as th,
    widgets::{
        device_chrome::section_label,
        gate_controls::{self, db},
        gate_display::EnvelopeHistory,
        level_meter::{LevelMeter, WIDTH as METER_WIDTH},
    },
};
use iced::{
    widget::{canvas, column, container, horizontal_space, row, text, tooltip},
    Color, Element, Length, Size,
};
use vibez_core::{effect::GatePhase, id::TrackId};
use vibez_engine::metering::MeterValues;

pub use crate::domains::gate_layout::card_width;

pub fn preset_bar_width(size: Size) -> f32 {
    gate_layout::preset_bar_width(size.width, size.height)
}

fn state_label(effect: &UiEffect, panel: &GatePanel) -> &'static str {
    if effect.bypass {
        "BYPASSED"
    } else {
        match panel
            .history
            .meter()
            .and_then(|meter| meter.gate)
            .map(|gate| gate.phase)
        {
            Some(GatePhase::Open) => "OPEN",
            Some(GatePhase::Hold) => "HOLD",
            Some(GatePhase::Closed) => "CLOSED",
            None => "--",
        }
    }
}

fn reduction(effect: &UiEffect, panel: &GatePanel) -> String {
    if effect.bypass {
        return "--".into();
    }
    panel.history.meter().map_or("--".into(), |meter| {
        format!(
            "{:.1}",
            meter.current_reduction_db[0].max(meter.current_reduction_db[1])
        )
    })
}

fn signal_meter<'a>(
    label: &'static str,
    levels: MeterValues,
    stereo_numbers: bool,
) -> Element<'a, Message> {
    let mut meter = column![
        section_label(label),
        canvas(LevelMeter { levels })
            .width(METER_WIDTH)
            .height(Length::Fill)
    ]
    .spacing(2)
    .height(Length::Fill)
    .width(Length::Fill);
    if stereo_numbers {
        for (channel, peak) in [("L", levels.peak_l), ("R", levels.peak_r)] {
            meter = meter.push(
                text(format!("{channel} {}", db(peak)))
                    .size(8)
                    .font(crate::typography::PERFORM_TECH)
                    .wrapping(iced::widget::text::Wrapping::None),
            );
        }
    } else {
        meter = meter.push(
            text(db(levels.peak_l.max(levels.peak_r)))
                .size(8)
                .font(crate::typography::PERFORM_TECH)
                .wrapping(iced::widget::text::Wrapping::None),
        );
    }
    meter.into()
}

fn state_readout<'a>(effect: &UiEffect, panel: &GatePanel, accent: Color) -> Element<'a, Message> {
    let gate = if effect.bypass {
        None
    } else {
        panel.history.meter().and_then(|meter| meter.gate)
    };
    let state_color = if gate.is_some_and(|gate| gate.phase != GatePhase::Closed) {
        accent
    } else {
        th::text_dim()
    };
    column![
        section_label("State"),
        text(state_label(effect, panel)).size(11).color(state_color),
        section_label("Wet GR dB"),
        text(reduction(effect, panel))
            .size(10)
            .font(crate::typography::PERFORM_TECH),
        text(gate.map_or("Env --".into(), |gate| format!(
            "Env {:.1} dB",
            gate.envelope_db
        )))
        .size(8)
        .color(th::text_dim()),
        text(gate.map_or("Hold --".into(), |gate| format!(
            "Hold {:.1} ms",
            gate.hold_remaining_ms
        )))
        .size(8)
        .color(th::text_dim()),
        text("Peak envelope").size(8).color(th::text_dim()),
        text("Open / close").size(8).color(accent),
        text("Wet GR").size(8).color(th::with_alpha(accent, 0.55)),
        text("O / H / C windows").size(8).color(th::text_dim())
    ]
    .spacing(3)
    .width(88.0)
    .into()
}

#[allow(clippy::too_many_arguments)]
pub fn view<'a>(
    track: TrackId,
    effect: &'a UiEffect,
    accent: Color,
    state: &'a DeviceControlState,
    panel: &'a GatePanel,
    presets: Element<'a, Message>,
    input: MeterValues,
    output: MeterValues,
    sources: &[ProjectTrack],
    buses: &[ProjectTrack],
    devices: &DevicesState,
    size: Size,
) -> Element<'a, Message> {
    let layout = gate_layout::arrange(size.width, size.height, state.advanced);
    let accent = if effect.bypass {
        th::text_muted()
    } else {
        accent
    };
    let disclosure =
        || gate_controls::disclosure(track, effect, state.advanced, layout.compact, accent);
    let advanced = || {
        gate_controls::advanced(
            track,
            effect,
            state,
            panel,
            accent,
            sources,
            buses,
            devices,
            input,
            output,
            layout.output_column,
        )
    };
    let mut toolbar = row![presets, horizontal_space()]
        .spacing(5)
        .align_y(iced::Alignment::Center);
    if size.height < 160.0 {
        toolbar = toolbar.push(
            text(format!(
                "{} GR{}dB",
                state_label(effect, panel),
                reduction(effect, panel)
            ))
            .size(9)
            .font(crate::typography::PERFORM_TECH)
            .wrapping(iced::widget::text::Wrapping::None),
        );
        if state.advanced && (!layout.primary_visible || layout.inline_details) {
            toolbar = toolbar.push(disclosure());
        }
    }
    let toolbar = container(toolbar).padding([0, 4]).height(layout.toolbar);
    let mut main = column![];
    if layout.plot > 0.0 {
        let short_state = layout.compact || layout.plot < 150.0;
        let mut header =
            row![section_label("Detector peak · snapshots")].align_y(iced::Alignment::Center);
        if short_state && !layout.output_column {
            header = header.push(horizontal_space()).push(
                text(format!(
                    "{} · GR {} dB",
                    state_label(effect, panel),
                    reduction(effect, panel)
                ))
                .size(9)
                .color(accent)
                .wrapping(iced::widget::text::Wrapping::None),
            );
        }
        let windows = if effect.bypass {
            Vec::new()
        } else {
            panel.history.windows().collect()
        };
        let graph = tooltip(canvas(EnvelopeHistory { track, effect, windows, accent }).width(Length::Fill).height(Length::Fill),
            text("Received detector snapshots, nominally 50 Hz. Window durations follow audio buffers; dropped snapshots are omitted. The horizontal axis counts received snapshots. Shading and the lower trace show peak wet attenuation. The three bands mark Open, Hold and Closed presence within each snapshot. Drag vertically to edit Threshold.").size(9),
            tooltip::Position::Top);
        let mut history = row![column![header, graph]
            .spacing(3)
            .width(Length::Fill)
            .height(Length::Fill)]
        .spacing(8)
        .height(Length::Fill);
        if !short_state {
            history = history.push(state_readout(effect, panel, accent));
        }
        main = main.push(container(history).height(layout.plot).padding([2, 4]));
    }
    let primary = || {
        gate_controls::primary(
            track,
            effect,
            state,
            accent,
            layout.knob_size,
            layout.primary_indices(),
        )
    };
    let controls: Element<'a, Message> = if layout.advanced_aside {
        row![
            primary(),
            iced::widget::vertical_rule(1),
            container(gate_controls::aside(
                track, effect, state, panel, accent, input, output
            ))
            .width(ASIDE_WIDTH)
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center)
        .into()
    } else if layout.inline_details {
        row![
            container(primary()).width(INLINE_CORE_WIDTH),
            iced::widget::vertical_rule(1),
            advanced()
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .into()
    } else if layout.primary_visible {
        row![primary(), disclosure()]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .into()
    } else {
        advanced()
    };
    main = main.push(iced::widget::horizontal_rule(1)).push(
        container(controls)
            .height(layout.controls - 1.0)
            .padding([2, 4])
            .align_y(iced::Alignment::Center),
    );
    if layout.details > 0.0 {
        main = main.push(
            container(advanced())
                .height(layout.details)
                .padding([3, 4])
                .align_y(iced::Alignment::Center),
        );
    }
    let mut content = row![].spacing(8).height(Length::Fill);
    if layout.source_column {
        let id = effect.id;
        let source = crate::widgets::native_sidechain::view(
            track,
            effect,
            sources,
            buses,
            devices,
            panel.sidechain_input,
            move |input| Message::GateControl(GateMsg::SidechainInput(track, id, input)),
            accent,
            true,
        );
        let extended_route = crate::domains::native_sidechain::presentation(
            effect,
            sources,
            buses,
            devices,
            panel.sidechain_input,
        )
        .is_some_and(|source| source.inputs.len() > 1 || source.warning.is_some());
        let meters: Element<'a, Message> = if size.height < 280.0 && extended_route {
            column![
                text(format!("In L {} R {}", db(input.peak_l), db(input.peak_r)))
                    .size(8)
                    .font(crate::typography::PERFORM_TECH)
                    .wrapping(iced::widget::text::Wrapping::None),
                text(format!(
                    "Out L {} R {}",
                    db(output.peak_l),
                    db(output.peak_r)
                ))
                .size(8)
                .font(crate::typography::PERFORM_TECH)
                .wrapping(iced::widget::text::Wrapping::None)
            ]
            .spacing(2)
            .into()
        } else {
            row![
                signal_meter("Input", input, true),
                signal_meter("Output", output, true)
            ]
            .spacing(8)
            .height(Length::Fill)
            .into()
        };
        content = content.push(
            container(column![source, meters].spacing(6).height(Length::Fill))
                .width(SOURCE_WIDTH)
                .padding([2, 4]),
        );
    }
    content = content.push(main.width(Length::Fill).height(Length::Fill));
    if layout.output_column {
        content = content.push(
            container(
                column![
                    column![
                        section_label("State"),
                        text(state_label(effect, panel)).size(11).color(accent),
                        section_label("Wet GR dB"),
                        text(reduction(effect, panel))
                            .size(10)
                            .font(crate::typography::PERFORM_TECH)
                    ]
                    .spacing(3),
                    row![
                        signal_meter("Input", input, true),
                        signal_meter("Output", output, true)
                    ]
                    .spacing(8)
                    .height(Length::Fill)
                ]
                .spacing(8)
                .height(Length::Fill),
            )
            .width(OUTPUT_WIDTH)
            .padding([2, 4]),
        );
    }
    column![toolbar, content]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_primary_children_fit_after_real_padding_and_separator() {
        for width in [430.0, 620.0, 740.0, 812.0, 872.0, 900.0] {
            for height in [64.0, 112.0, 146.0, 160.0, 240.0, 279.0, 280.0, 304.0] {
                for advanced in [false, true] {
                    let layout = gate_layout::arrange(width, height, advanced);
                    let field = 8.0 * 1.3 + 3.0 + 10.0 * 1.3 + 2.0;
                    let child = field + layout.knob_size.map_or(0.0, |size| size + 3.0);
                    assert!(
                        layout.controls - 5.0 >= child,
                        "primary clips at {width}x{height}"
                    );
                    if layout.advanced_aside {
                        let bank = width - SOURCE_WIDTH - 8.0 - 8.0 - ASIDE_WIDTH - 1.0 - 24.0;
                        assert!((bank - 4.0 * 6.0) / 5.0 >= 68.0);
                        let header = 9.0 * 1.3 + 6.0;
                        assert!(layout.controls - 5.0 >= header + 3.0 + 2.0 * field + 4.0);
                    }
                    if layout.source_column {
                        let three_fields = 3.0 * field;
                        let disconnect_warning_raw = 9.0 * 1.3 + 4.0 + 2.0 * 8.0 * 1.3;
                        let source = three_fields + disconnect_warning_raw + 5.0 * 5.0;
                        let numeric_io = 2.0 * 8.0 * 1.3 + 2.0;
                        assert!(height - layout.toolbar - 4.0 >= source + 6.0 + numeric_io);
                    }
                }
            }
        }
    }
}
