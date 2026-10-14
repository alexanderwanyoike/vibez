//! Measured EQ layouts keep the response, selected editor and divided band console in proportion.

use crate::{
    domains::{device_controls::DeviceControlState, eq::EqPanel},
    message::Message,
    state::UiEffect,
    theme as th,
    widgets::{
        device_chrome::section_label, device_controls::disclosure, eq_device_controls::Controls,
        eq_display::EqDisplayWidget,
    },
};
use iced::{
    widget::{canvas, column, container, horizontal_space, row},
    Color, Element, Length, Size,
};
use vibez_core::id::TrackId;
use vibez_engine::metering::MeterValues;

pub const COMPACT_W: f32 = 430.0;
pub const MEDIUM_W: f32 = 740.0;
pub const MAX_W: f32 = 900.0;
const TOOLBAR_H: f32 = 26.0;

pub fn card_width(available: f32) -> f32 {
    available.clamp(COMPACT_W, MAX_W)
}
pub fn preset_bar_width(size: Size) -> f32 {
    if size.height >= 220.0 && size.width >= MEDIUM_W && size.width < 872.0 {
        size.width - 170.0
    } else {
        size.width - 106.0
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Arrangement {
    pub graph: f32,
    pub controls: f32,
    pub details: f32,
    pub all_bands: bool,
    pub expanded: bool,
    pub tabs: bool,
    pub side_editor: bool,
}

pub fn arrange(size: Size, advanced: bool) -> Arrangement {
    let full = size.height >= 220.0;
    let controls = if full { 94.0 } else { 36.0 };
    let details = if advanced && full { 24.0 } else { 0.0 };
    let graph = if size.height < 114.0 {
        0.0
    } else {
        (size.height - TOOLBAR_H - controls - details).max(0.0)
    };
    let all_bands = full && size.width >= MEDIUM_W;
    Arrangement {
        graph,
        controls,
        details,
        all_bands,
        expanded: all_bands && size.width >= 872.0,
        tabs: full && !all_bands,
        side_editor: all_bands && graph >= 160.0,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn view<'a>(
    track: TrackId,
    effect: &'a UiEffect,
    accent: Color,
    state: &'a DeviceControlState,
    panel: &'a EqPanel,
    analyser_active: bool,
    preset: Element<'a, Message>,
    input: MeterValues,
    output: MeterValues,
    sample_rate: f32,
    size: Size,
) -> Element<'a, Message> {
    let a = arrange(size, state.advanced);
    let accent = if effect.bypass {
        th::text_muted()
    } else {
        accent
    };
    let c = Controls {
        track,
        effect,
        state,
        panel,
        accent,
        analyser_active,
    };
    let mut toolbar = row![preset, horizontal_space()]
        .align_y(iced::Alignment::Center)
        .height(TOOLBAR_H);
    if !a.tabs && !a.all_bands {
        toolbar = toolbar.push(disclosure(track, effect, state.advanced));
    } else if a.all_bands && !a.expanded {
        toolbar = toolbar.push(c.analyser());
    }
    let mut body = column![toolbar];
    if a.graph > 0.0 {
        let graph = canvas(EqDisplayWidget {
            track_id: track,
            effect_id: effect.id,
            params: effect.params.clone(),
            descriptors: effect.descriptors,
            bypass: effect.bypass,
            accent,
            selected_band: panel.selected_band,
            sample_rate,
            spectrum: panel.spectrum.bins.clone(),
            spectrum_active: panel.spectrum.active,
        })
        .width(Length::Fill)
        .height(Length::Fill);
        let caption: Element<'a, Message> = if !a.all_bands && !a.tabs {
            row![
                section_label("Response · manual values"),
                horizontal_space(),
                c.analyser()
            ]
            .align_y(iced::Alignment::Center)
            .into()
        } else {
            section_label("Response · manual values")
        };
        let response = column![caption, graph].spacing(3).height(Length::Fill);
        let mut plot = row![response].height(Length::Fill);
        if a.side_editor {
            plot = plot
                .push(iced::widget::vertical_rule(1))
                .push(c.editor())
                .spacing(6);
        }
        body = body.push(container(plot).height(a.graph).padding([3, 4]));
    }
    let controls = if a.all_bands {
        c.console(input, output, a.expanded, a.side_editor)
    } else if a.tabs {
        column![c.selected(true, true), c.utility_row()]
            .spacing(7)
            .into()
    } else if state.advanced && size.width >= MEDIUM_W {
        row![c.selected(false, false), c.advanced()]
            .spacing(12)
            .into()
    } else {
        c.short()
    };
    body = body.push(iced::widget::horizontal_rule(1)).push(
        container(controls)
            .padding([if a.all_bands || a.tabs { 4 } else { 1 }, 4])
            .height(a.controls - 1.0),
    );
    if a.details > 0.0 {
        body = body.push(container(c.advanced()).padding([2, 4]).height(a.details));
    }
    body.width(Length::Fill).height(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_measured_tier_fits_actual_minimum_body_without_internal_scrolling() {
        for width in [430.0, 620.0, 740.0, 804.0, 892.0] {
            for height in [
                64.0, 72.0, 90.0, 108.0, 113.0, 114.0, 130.0, 160.0, 219.0, 220.0, 240.0, 304.0,
                400.0,
            ] {
                for advanced in [false, true] {
                    let a = arrange(Size::new(width, height), advanced);
                    assert!(
                        TOOLBAR_H + a.controls + a.graph + a.details <= height + 0.001,
                        "{width}x{height}, advanced={advanced}"
                    );
                    assert!(a.graph == 0.0 || a.graph >= 48.0);
                    assert!(a.controls >= if a.all_bands || a.tabs { 94.0 } else { 36.0 });
                    assert_eq!(a.side_editor, a.all_bands && a.graph >= 160.0);
                    assert_eq!(a.details > 0.0, advanced && height >= 220.0);
                }
            }
        }
    }
    #[test]
    fn compact_reveals_four_tabs_and_contextual_controls_without_duplicate_side_editor() {
        let a = arrange(Size::new(472.0, 304.0), false);
        assert!(a.tabs);
        assert!(!a.all_bands);
        assert!(!a.side_editor);
        assert_eq!(a.controls, 94.0);
    }
    #[test]
    fn band_groups_have_room_for_three_bounded_parameter_columns() {
        for width in [740.0, 804.0, 872.0, 892.0] {
            let a = arrange(Size::new(width, 304.0), false);
            let utility = if a.expanded { 267.0 } else { 106.0 };
            let band = (width - 8.0 - 17.0 - utility - 51.0) / 4.0;
            assert!(
                (band - 10.0) / 3.0 >= 40.0,
                "band fields too narrow at {width}"
            );
        }
    }
    #[test]
    fn medium_and_expanded_keep_the_four_band_console_when_advanced_opens() {
        for width in [804.0, 892.0] {
            for advanced in [false, true] {
                let layout = arrange(Size::new(width, 304.0), advanced);
                assert!(
                    layout.all_bands,
                    "missing band console at {width}, advanced={advanced}"
                );
                assert!(
                    layout.graph >= 160.0,
                    "response and side editor lost their space"
                );
            }
        }
    }

    #[test]
    fn width_uses_chain_remainder_and_is_bounded() {
        assert_eq!(card_width(80.0), 430.0);
        assert_eq!(card_width(710.0), 710.0);
        assert_eq!(card_width(1800.0), 900.0);
    }
}
