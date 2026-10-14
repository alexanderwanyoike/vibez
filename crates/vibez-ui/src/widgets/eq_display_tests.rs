//! Exercises actual EQ pointer events without a rendered window.

use super::*;
use crate::domains::devices::DevicesMsg;
use iced::widget::canvas::Program;
use vibez_dsp::effect::AudioEffect;

#[test]
fn wheel_edits_only_mid_band_q_and_preserves_shelf_bell_modes() {
    let effect = vibez_dsp::eq::EqEffect::new(48_000.0);
    let descriptors = effect.param_descriptors();
    let display = EqDisplayWidget {
        track_id: TrackId::new(),
        effect_id: EffectId::new(),
        params: descriptors.iter().map(|d| d.default).collect(),
        descriptors,
        bypass: false,
        sample_rate: 48_000.0,
        spectrum: Vec::new(),
        spectrum_active: false,
    };
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(400.0, 180.0));
    for (index, band) in BANDS.iter().enumerate() {
        let cursor = mouse::Cursor::Available(display.handle_pos(band, bounds));
        let (status, message) = display.update(
            &mut EqDisplayState::default(),
            canvas::Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
            }),
            bounds,
            cursor,
        );
        assert_eq!(status, canvas::event::Status::Captured);
        match index {
            1 | 2 => assert!(
                matches!(message, Some(Message::Devices(DevicesMsg::SetEffectParam(_, _, param, value))) if param == [5, 8][index - 1] && value > 0.7)
            ),
            _ => assert!(
                message.is_none(),
                "shelf/bell band {index} was treated as Q"
            ),
        }
    }
}
