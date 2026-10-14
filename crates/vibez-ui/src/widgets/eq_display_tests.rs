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
        accent: Color::WHITE,
        selected_band: 0,
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

#[test]
fn pointer_selects_the_band_and_one_drag_shares_a_single_undo_gesture() {
    let effect = vibez_dsp::eq::EqEffect::new(48_000.0);
    let descriptors = effect.param_descriptors();
    let display = EqDisplayWidget {
        track_id: TrackId::new(),
        effect_id: EffectId::new(),
        params: descriptors.iter().map(|d| d.default).collect(),
        descriptors,
        bypass: false,
        accent: Color::WHITE,
        selected_band: 0,
        sample_rate: 48_000.0,
        spectrum: vec![],
        spectrum_active: false,
    };
    let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(400.0, 180.0));
    let mut state = EqDisplayState::default();
    let start = display.handle_pos(&BANDS[2], bounds);
    let (_, select) = display.update(
        &mut state,
        canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        bounds,
        mouse::Cursor::Available(start),
    );
    assert!(
        matches!(select,Some(Message::EqControl(crate::domains::eq::EqMsg::SelectBand(track,effect,2))) if track == display.track_id && effect == display.effect_id)
    );
    let mut gesture = None;
    for offset in [8.0, 16.0] {
        let point = Point::new(start.x + offset, start.y - offset);
        let (_, message) = display.update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::CursorMoved { position: point }),
            bounds,
            mouse::Cursor::Available(point),
        );
        let Some(Message::UndoGesture { id, edit }) = message else {
            panic!("drag must carry one gesture");
        };
        assert!(gesture.is_none_or(|previous| previous == id));
        gesture = Some(id);
        assert!(
            matches!(*edit,Message::Devices(DevicesMsg::SetEffectParams(_,_,ref updates)) if updates.len()==2 && updates[0].0 == 7 && updates[1].0 ==6)
        );
    }
    display.update(
        &mut state,
        canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        bounds,
        mouse::Cursor::Available(start),
    );
    assert!(state.drag.is_none());
    assert!(state.undo_gesture.is_none());
}
