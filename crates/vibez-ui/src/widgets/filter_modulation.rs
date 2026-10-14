//! Filter modulation controls with explicitly modelled sine and envelope previews.

use super::{flat_button, value};
use crate::{
    domains::{device_controls::DeviceControlState, filter_layout::Region},
    message::Message,
    state::UiEffect,
    theme as th,
    widgets::{
        device_chrome::section_label,
        device_controls::{compact_value_field, disclosure, parameter, selector},
    },
};
use iced::{
    mouse,
    widget::{canvas, column, container, row, text},
    Alignment, Color, Element, Length, Point, Rectangle, Renderer, Theme,
};
use vibez_core::id::TrackId;
use vibez_dsp::filter::{param, DEPTH_MAX_OCTAVES};

pub fn view<'a>(
    track: TrackId,
    effect: &'a UiEffect,
    state: &'a DeviceControlState,
    accent: Color,
    region: Region,
    inline_disclosure: bool,
) -> Element<'a, Message> {
    let source = value(effect, param::MOD_SOURCE).round() as usize;
    let dense = region.height < 52.0;
    let mut sources = row![].spacing(3);
    let source_labels = if dense {
        ["Off", "LFO", "Env"]
    } else {
        ["Off", "LFO", "Envelope"]
    };
    for (index, label) in source_labels.into_iter().enumerate() {
        sources = sources.push(
            flat_button(
                label,
                accent,
                index == source,
                Some(Message::set_effect_param(
                    track,
                    effect.id,
                    param::MOD_SOURCE,
                    index as f32,
                )),
            )
            .height(if dense { 14.0 } else { 20.0 }),
        );
    }
    let mut settings = row![]
        .spacing(if dense { 4 } else { 8 })
        .align_y(Alignment::Center);
    let mut indices = vec![param::DEPTH];
    if source == 2 {
        indices.extend([param::ENV_ATTACK, param::ENV_RELEASE]);
    } else {
        indices.extend([param::RATE, param::STEREO_PHASE]);
    }
    let depth_knob = inline_disclosure && region.height >= 68.0;
    for index in indices {
        if depth_knob && index == param::DEPTH {
            continue;
        }
        let control: Element<'a, Message> = if dense {
            row![
                text(match index {
                    param::DEPTH => "Oct",
                    param::RATE => "Hz",
                    param::STEREO_PHASE => "°",
                    param::ENV_ATTACK => "A",
                    _ => "R",
                })
                .size(8)
                .color(th::text_dim()),
                compact_value_field(track, effect, state, index, accent)
            ]
            .spacing(2)
            .align_y(Alignment::Center)
            .into()
        } else {
            parameter(track, effect, state, index, accent, None)
        };
        settings = settings.push(control);
    }
    if source != 2 {
        settings = settings.push(if dense {
            // A native selector keeps every sync division reachable in the shortest body.
            crate::widgets::filter_device::sync_choice(track, effect, accent)
        } else {
            selector(
                track,
                effect,
                param::SYNC,
                "Sync",
                &["Free", "1 bar", "1/2", "1/4", "1/8", "1/16", "1/4T", "1/8D"],
                accent,
            )
        });
    }
    if dense {
        let mut controls = row![].spacing(6).height(Length::Fill).align_y(Alignment::Center);
        if inline_disclosure {
            controls = controls.push(container(disclosure(track, effect, true)).width(82.0));
        }
        return controls.push(container(sources).width(132.0)).push(settings).into();
    }
    let header: Element<'a, Message> = if inline_disclosure {
        row![
            container(disclosure(track, effect, true)).width(82.0),
            sources
        ]
        .spacing(6)
        .into()
    } else {
        sources.into()
    };
    let controls = column![header, settings].spacing(4).width(Length::Fill);
    if region.height < 52.0 {
        return controls.into();
    }
    let amount = value(effect, param::DEPTH);
    let depth_readout = match source {
        1 => format!("±{amount:.2}oct"),
        2 => format!("+{amount:.2}oct"),
        _ => "0oct".into(),
    };
    let preview = column![
        row![section_label(match source {
            1 => "Model LFO", 2 => "Model step", _ => "Model off",
        }), text(depth_readout).size(8).color(th::text_dim())].spacing(4),
        canvas(Preview { effect, accent })
            .width(Length::Fill)
            .height(Length::Fill)
    ]
    .spacing(3)
    .width(if inline_disclosure { 128.0 } else { 96.0 })
    .height(Length::Fill);
    let mut content = row![controls]
        .spacing(12)
        .align_y(Alignment::Center)
        .height(Length::Fill);
    if depth_knob {
        content = content.push(
            container(parameter(
                track,
                effect,
                state,
                param::DEPTH,
                accent,
                Some(38.0),
            ))
            .width(66.0),
        );
    }
    content.push(preview).into()
}

fn model_value(
    source: usize,
    position: f32,
    depth: f32,
    phase_degrees: f32,
    attack_ms: f32,
    release_ms: f32,
) -> f32 {
    let amount = depth / DEPTH_MAX_OCTAVES;
    match source {
        1 => (std::f32::consts::TAU * (position * 2.0 + phase_degrees / 360.0)).sin() * amount,
        2 => {
            let hold = attack_ms * 3.0;
            let time = position * (hold + release_ms * 5.0);
            if time <= hold {
                (1.0 - (-time / attack_ms).exp()) * amount
            } else {
                (1.0 - (-hold / attack_ms).exp()) * (-(time - hold) / release_ms).exp() * amount
            }
        }
        _ => 0.0,
    }
}

struct Preview<'a> {
    effect: &'a UiEffect,
    accent: Color,
}
impl canvas::Program<Message> for Preview<'_> {
    type State = ();
    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), th::bg_dark());
        let source = value(self.effect, param::MOD_SOURCE).round() as usize;
        let center = bounds.height * if source == 2 { 0.9 } else { 0.5 };
        let span = bounds.height * if source == 2 { 0.8 } else { 0.42 };
        frame.stroke(
            &canvas::Path::line(Point::new(0.0, center), Point::new(bounds.width, center)),
            canvas::Stroke::default().with_color(th::divider()),
        );
        frame.fill_text(canvas::Text {
            content: if source == 2 { "0..6 oct" } else { "±6 oct" }.into(),
            position: Point::new(2.0, 2.0), color: th::text_dim(), size: 7.0.into(),
            ..Default::default()
        });
        for channel in [1, 0] {
            let path = canvas::Path::new(|builder| {
                for index in 0..128 {
                    let position = index as f32 / 127.0;
                    let amplitude = model_value(
                        source,
                        position,
                        value(self.effect, param::DEPTH),
                        channel as f32 * value(self.effect, param::STEREO_PHASE),
                        value(self.effect, param::ENV_ATTACK),
                        value(self.effect, param::ENV_RELEASE),
                    );
                    let point = Point::new(position * bounds.width, center - amplitude * span);
                    if index == 0 {
                        builder.move_to(point);
                    } else {
                        builder.line_to(point);
                    }
                }
            });
            frame.stroke(
                &path,
                canvas::Stroke::default()
                    .with_width(1.5)
                    .with_color(th::with_alpha(
                        self.accent,
                        if channel == 0 { 1.0 } else { 0.35 },
                    )),
            );
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shape_preview_uses_sine_depth_and_stereo_phase_without_claiming_live_phase() {
        let sample = |position, depth, phase| model_value(1, position, depth, phase, 10.0, 150.0);
        assert_eq!(sample(0.0, 6.0, 0.0), 0.0);
        assert!((sample(0.125, 6.0, 0.0) - 1.0).abs() < 1e-6);
        assert!((sample(0.125, 3.0, 0.0) - 0.5).abs() < 1e-6);
        assert!((sample(0.125, 6.0, 180.0) + 1.0).abs() < 1e-6);
        assert_eq!(sample(0.125, 0.0, 0.0), 0.0);
        assert_eq!(model_value(0, 0.125, 6.0, 0.0, 10.0, 150.0), 0.0);
    }
    #[test]
    fn envelope_preview_matches_the_attack_and_release_time_constants() {
        let (attack, release) = (10.0, 150.0);
        let duration = 3.0 * attack + 5.0 * release;
        let at_attack = model_value(2, attack / duration, 6.0, 0.0, attack, release);
        assert!((at_attack - (1.0 - (-1.0_f32).exp())).abs() < 1e-6);
        let after_release = model_value(
            2,
            (3.0 * attack + release) / duration,
            6.0,
            0.0,
            attack,
            release,
        );
        assert!((after_release - (1.0 - (-3.0_f32).exp()) * (-1.0_f32).exp()).abs() < 1e-6);
    }
}
