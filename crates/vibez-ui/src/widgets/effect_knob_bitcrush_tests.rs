//! Verifies native Bits/Rate gestures through the actual shared canvas event API.

use super::*;
use crate::domains::devices::DevicesMsg;
use iced::{Point, Size};
use vibez_dsp::bitcrush::param;

#[derive(Clone, Copy, Debug)]
enum Kind {
    Bits,
    Rate,
}

fn knob(kind: Kind, value: f32) -> EffectKnobWidget {
    let effect = EffectId::new();
    let index = match kind {
        Kind::Bits => param::BITS,
        Kind::Rate => param::SR_REDUCE,
    };
    let descriptor = vibez_dsp::bitcrush::BITCRUSH_PARAMS[index];
    let mut knob = EffectKnobWidget::new(
        TrackId::new(),
        effect,
        index,
        value,
        descriptor.min,
        descriptor.max,
        descriptor.default,
        Color::WHITE,
    );
    knob.target = match kind {
        Kind::Bits => KnobTarget::BitcrushBits(effect),
        Kind::Rate => KnobTarget::BitcrushRate(effect),
    };
    knob
}

fn at(x: f32, y: f32) -> mouse::Cursor {
    mouse::Cursor::Available(Point::new(x, y))
}

fn event(
    knob: &EffectKnobWidget,
    state: &mut EffectKnobState,
    event: canvas::Event,
    cursor: mouse::Cursor,
) -> (canvas::event::Status, Option<Message>) {
    <EffectKnobWidget as canvas::Program<Message>>::update(
        knob,
        state,
        event,
        Rectangle::new(Point::ORIGIN, Size::new(40.0, 40.0)),
        cursor,
    )
}

fn wheel(
    knob: &EffectKnobWidget,
    state: &mut EffectKnobState,
    delta: mouse::ScrollDelta,
) -> Option<Message> {
    let (status, message) = event(
        knob,
        state,
        canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }),
        at(20.0, 20.0),
    );
    assert_eq!(status, canvas::event::Status::Captured);
    message
}

fn lines(knob: &EffectKnobWidget, state: &mut EffectKnobState, y: f32) -> Option<f32> {
    value(
        knob,
        wheel(knob, state, mouse::ScrollDelta::Lines { x: 0.0, y }),
    )
}

fn pixels(knob: &EffectKnobWidget, state: &mut EffectKnobState, y: f32) -> Option<f32> {
    value(
        knob,
        wheel(knob, state, mouse::ScrollDelta::Pixels { x: 0.0, y }),
    )
}

fn value(knob: &EffectKnobWidget, message: Option<Message>) -> Option<f32> {
    match message {
        Some(Message::UndoGesture { edit, .. }) => value(knob, Some(*edit)),
        Some(Message::Devices(DevicesMsg::SetEffectParam(track, effect, index, value))) => {
            assert_eq!(track, knob.track_id);
            let expected = match knob.target {
                KnobTarget::BitcrushBits(effect) | KnobTarget::BitcrushRate(effect) => effect,
                _ => panic!("expected native Bitcrush target"),
            };
            assert_eq!(effect, expected);
            assert_eq!(index, knob.param_index);
            Some(value)
        }
        None => None,
        other => panic!("unexpected stepped message {other:?}"),
    }
}

#[test]
fn native_bitcrush_arcs_and_inverse_use_effective_bits_and_log_nominal_hertz() {
    let bits = knob(Kind::Bits, 8.75);
    assert!((bits.normalized() - 6.0 / 14.0).abs() < 1e-6);
    assert_eq!(
        bits.denormalize(bits.normalized()).to_bits(),
        8.75_f32.to_bits()
    );
    for selected in 2..=16 {
        let expected = if selected == 8 { 8.75 } else { selected as f32 };
        let normalized = (selected - 2) as f32 / 14.0;
        assert_eq!(bits.denormalize(normalized).to_bits(), expected.to_bits());
    }
    let rate = knob(Kind::Rate, 4.49);
    assert!((rate.normalized() - 0.6).abs() < 1e-6);
    assert_eq!(
        rate.denormalize(rate.normalized()).to_bits(),
        4.49_f32.to_bits()
    );
    for divider in 1..=32 {
        let position = ((32.0_f64 / f64::from(divider)).ln() / 32.0_f64.ln()) as f32;
        let expected = if divider == 4 { 4.49 } else { divider as f32 };
        assert_eq!(
            rate.denormalize(position).to_bits(),
            expected.to_bits(),
            "N{divider}"
        );
    }
    assert_eq!(rate.denormalize(0.5), 6.0);
    assert_eq!(rate.denormalize(0.0), 32.0);
    assert_eq!(rate.denormalize(1.0), 1.0);
}

#[test]
fn native_bitcrush_wheel_accumulates_steps_before_redraw_and_has_no_boundary_debt() {
    for (kind, current, higher, twice_higher, minimum, maximum, above_minimum, below_maximum) in [
        (Kind::Bits, 8.75, 9.0, 10.0, 2.0, 16.0, 3.0, 15.0),
        (Kind::Rate, 4.49, 3.0, 2.0, 32.0, 1.0, 31.0, 2.0),
    ] {
        let knob = knob(kind, current);
        let mut state = EffectKnobState::default();
        assert_eq!(lines(&knob, &mut state, 1.0), Some(higher), "{kind:?}");
        assert_eq!(
            lines(&knob, &mut state, 1.0),
            Some(twice_higher),
            "batched {kind:?}"
        );
        assert_eq!(lines(&knob, &mut state, -1.0), Some(higher));
        assert_eq!(
            lines(&knob, &mut state, -1.0).unwrap().to_bits(),
            current.to_bits()
        );
        assert_eq!(lines(&knob, &mut state, 0.0), None);

        let mut state = EffectKnobState::default();
        for _ in 0..3 {
            assert_eq!(pixels(&knob, &mut state, 5.0), None);
        }
        assert_eq!(
            pixels(&knob, &mut state, 5.0),
            Some(higher),
            "20px/line {kind:?}"
        );
        let mut state = EffectKnobState::default();
        assert_eq!(lines(&knob, &mut state, 0.5), None);
        assert_eq!(pixels(&knob, &mut state, 10.0), Some(higher));
        let mut state = EffectKnobState::default();
        event(
            &knob,
            &mut state,
            canvas::Event::Keyboard(keyboard::Event::ModifiersChanged(
                keyboard::Modifiers::SHIFT,
            )),
            at(20.0, 20.0),
        );
        assert_eq!(lines(&knob, &mut state, 4.0), None);
        assert_eq!(
            lines(&knob, &mut state, 1.0),
            Some(higher),
            "five Shift lines {kind:?}"
        );
        event(
            &knob,
            &mut state,
            canvas::Event::Keyboard(keyboard::Event::ModifiersChanged(
                keyboard::Modifiers::empty(),
            )),
            at(20.0, 20.0),
        );
        assert_eq!(lines(&knob, &mut state, 1.0), Some(twice_higher));

        let mut state = EffectKnobState::default();
        assert_eq!(lines(&knob, &mut state, 0.75), None);
        assert_eq!(lines(&knob, &mut state, -0.5), None);
        assert_eq!(lines(&knob, &mut state, 0.75), Some(higher));

        let mut edge = knob;
        edge.value = maximum;
        let mut state = EffectKnobState::default();
        assert_eq!(lines(&edge, &mut state, 100.0), None);
        assert_eq!(lines(&edge, &mut state, -0.5), None);
        assert_eq!(lines(&edge, &mut state, -0.5), Some(below_maximum));
        let mut state = EffectKnobState::default();
        assert_eq!(lines(&edge, &mut state, 0.75), None);
        assert_eq!(lines(&edge, &mut state, -1.0), Some(below_maximum));
        edge.value = minimum;
        let mut state = EffectKnobState::default();
        assert_eq!(lines(&edge, &mut state, -100.0), None);
        assert_eq!(lines(&edge, &mut state, 1.0), Some(above_minimum));
        let mut state = EffectKnobState::default();
        assert_eq!(lines(&edge, &mut state, -0.75), None);
        assert_eq!(lines(&edge, &mut state, 1.0), Some(above_minimum));
    }
    for (kind, minimum, maximum, direction) in [(Kind::Bits, 2, 16, 1), (Kind::Rate, 1, 32, -1)] {
        for selected in minimum..=maximum {
            let control = knob(kind, selected as f32);
            for motion in [-1, 1] {
                let expected = selected + motion * direction;
                let mut state = EffectKnobState::default();
                let actual = lines(&control, &mut state, motion as f32);
                if (minimum..=maximum).contains(&expected) {
                    assert_eq!(
                        actual,
                        Some(expected as f32),
                        "adjacent {kind:?} {selected}"
                    );
                } else {
                    assert_eq!(actual, None, "boundary {kind:?} {selected}");
                }
            }
        }
    }
}

#[test]
fn native_bitcrush_wheel_acknowledgement_preserves_fractional_carry_across_redraws() {
    for (kind, current, higher, twice_higher) in [
        (Kind::Bits, 8.75, 9.0, 10.0),
        (Kind::Rate, 4.49, 3.0, 2.0),
    ] {
        for pixel_delta in [false, true] {
            for shifted in [false, true] {
                let mut control = knob(kind, current);
                let mut batched = EffectKnobState::default();
                let mut acknowledged = EffectKnobState::default();
                if shifted {
                    for state in [&mut batched, &mut acknowledged] {
                        event(
                            &control,
                            state,
                            canvas::Event::Keyboard(keyboard::Event::ModifiersChanged(
                                keyboard::Modifiers::SHIFT,
                            )),
                            at(20.0, 20.0),
                        );
                    }
                }
                let units_per_step = if pixel_delta { 20.0 } else { 1.0 }
                    * if shifted { 5.0 } else { 1.0 };
                let scroll = |control: &EffectKnobWidget,
                              state: &mut EffectKnobState,
                              steps: f32| {
                    let delta = steps * units_per_step;
                    if pixel_delta {
                        pixels(control, state, delta)
                    } else {
                        lines(control, state, delta)
                    }
                };
                let expected = scroll(&control, &mut batched, 2.0);
                assert_eq!(expected, Some(twice_higher));
                let emitted = scroll(&control, &mut acknowledged, 1.5);
                assert_eq!(emitted, Some(higher));
                control.value = emitted.unwrap();
                assert_eq!(
                    scroll(&control, &mut acknowledged, 0.5),
                    expected,
                    "same-target redraw {kind:?}, pixels={pixel_delta}, Shift={shifted}"
                );
            }
        }
    }
}

#[test]
fn native_bitcrush_fractional_wheel_carry_cannot_cross_exit_target_or_value_replacement() {
    for (kind, current, next, replacement_value) in [
        (Kind::Bits, 8.75, 9.0, 12.75),
        (Kind::Rate, 4.49, 3.0, 8.49),
    ] {
        let original = knob(kind, current);
        let mut state = EffectKnobState::default();
        assert_eq!(lines(&original, &mut state, 0.75), None);
        event(
            &original,
            &mut state,
            canvas::Event::Mouse(mouse::Event::CursorMoved {
                position: Point::new(80.0, 20.0),
            }),
            at(80.0, 20.0),
        );
        assert_eq!(lines(&original, &mut state, 0.25), None);
        assert_eq!(lines(&original, &mut state, 0.75), Some(next));

        let mut state = EffectKnobState::default();
        assert_eq!(lines(&original, &mut state, 0.75), None);
        event(
            &original,
            &mut state,
            canvas::Event::Mouse(mouse::Event::CursorLeft),
            mouse::Cursor::Unavailable,
        );
        assert_eq!(lines(&original, &mut state, 0.25), None);

        for replacement in [knob(kind, current), knob(kind, replacement_value)] {
            let mut state = EffectKnobState::default();
            assert_eq!(lines(&original, &mut state, 0.75), None);
            assert_eq!(
                lines(&replacement, &mut state, 0.25),
                None,
                "new effect target {kind:?}"
            );
        }
        let mut replacement = knob(kind, replacement_value);
        replacement.track_id = original.track_id;
        replacement.target = original.target;
        let mut state = EffectKnobState::default();
        assert_eq!(lines(&original, &mut state, 0.75), None);
        assert_eq!(
            lines(&replacement, &mut state, 0.25),
            None,
            "same target new value {kind:?}"
        );

        let mut replacement = knob(kind, current);
        replacement.target = original.target;
        let mut state = EffectKnobState::default();
        assert_eq!(lines(&original, &mut state, 0.75), None);
        assert_eq!(
            lines(&replacement, &mut state, 0.25),
            None,
            "new track {kind:?}"
        );
    }
    let bits = knob(Kind::Bits, 8.75);
    let mut rate = knob(Kind::Rate, 4.49);
    rate.track_id = bits.track_id;
    rate.target = match bits.target {
        KnobTarget::BitcrushBits(effect) => KnobTarget::BitcrushRate(effect),
        _ => unreachable!(),
    };
    let mut state = EffectKnobState::default();
    assert_eq!(lines(&bits, &mut state, 0.75), None);
    assert_eq!(lines(&rate, &mut state, 0.25), None);
}

#[test]
fn native_bitcrush_drag_accumulates_snaps_preserves_noops_and_resets_original_defaults() {
    for (kind, current, next, farther, distance) in [
        (Kind::Bits, 8.75, 9.0, 10.0, 20.0),
        (Kind::Rate, 4.49, 3.0, 2.0, 30.0),
    ] {
        let knob = knob(kind, current);
        let mut state = EffectKnobState::default();
        event(
            &knob,
            &mut state,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            at(20.0, 20.0),
        );
        assert!(
            event(
                &knob,
                &mut state,
                canvas::Event::Mouse(mouse::Event::CursorMoved {
                    position: Point::new(20.0, 19.0),
                }),
                at(20.0, 19.0)
            )
            .1
            .is_none(),
            "same effective value {kind:?}"
        );
        let first = event(
            &knob,
            &mut state,
            canvas::Event::Mouse(mouse::Event::CursorMoved {
                position: Point::new(20.0, 10.0),
            }),
            at(20.0, 10.0),
        )
        .1;
        let first_gesture = match &first {
            Some(Message::UndoGesture { id, .. }) => *id,
            other => panic!("drag needs one undo gesture: {other:?}"),
        };
        assert_eq!(value(&knob, first), Some(next));
        let moved = Point::new(20.0, 20.0 - distance);
        let second = event(
            &knob,
            &mut state,
            canvas::Event::Mouse(mouse::Event::CursorMoved { position: moved }),
            mouse::Cursor::Available(moved),
        )
        .1;
        assert!(matches!(&second, Some(Message::UndoGesture { id, .. }) if *id == first_gesture));
        assert_eq!(value(&knob, second), Some(farther), "batched drag {kind:?}");
        let returned = event(
            &knob,
            &mut state,
            canvas::Event::Mouse(mouse::Event::CursorMoved {
                position: Point::new(20.0, 20.0),
            }),
            at(20.0, 20.0),
        )
        .1;
        assert_eq!(value(&knob, returned).unwrap().to_bits(), current.to_bits());
        assert!(event(
            &knob,
            &mut state,
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            at(20.0, 20.0)
        )
        .1
        .is_none());

        let mut fine = EffectKnobState::default();
        event(
            &knob,
            &mut fine,
            canvas::Event::Keyboard(keyboard::Event::ModifiersChanged(
                keyboard::Modifiers::SHIFT,
            )),
            at(20.0, 20.0),
        );
        event(
            &knob,
            &mut fine,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            at(20.0, 20.0),
        );
        assert!(event(
            &knob,
            &mut fine,
            canvas::Event::Mouse(mouse::Event::CursorMoved {
                position: Point::new(20.0, 10.0),
            }),
            at(20.0, 10.0)
        )
        .1
        .is_none());
        let fine_move = event(
            &knob,
            &mut fine,
            canvas::Event::Mouse(mouse::Event::CursorMoved {
                position: Point::new(20.0, -30.0),
            }),
            at(20.0, -30.0),
        )
        .1;
        assert_eq!(
            value(&knob, fine_move),
            Some(next),
            "fivefold fine drag {kind:?}"
        );

        let mut reset = EffectKnobState::default();
        event(
            &knob,
            &mut reset,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            at(20.0, 20.0),
        );
        event(
            &knob,
            &mut reset,
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            at(20.0, 20.0),
        );
        let message = event(
            &knob,
            &mut reset,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            at(20.0, 20.0),
        )
        .1;
        assert_eq!(value(&knob, message), Some(knob.default));
    }
}
