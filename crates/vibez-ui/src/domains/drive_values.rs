//! Native Drive presentation preserves the original stored nonlinear amount and linear gain.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drive_decibels_invert_the_original_gain_formula() {
        for amount in [0.0, 0.01, 0.185, 0.35, 0.7, 1.0] {
            let db = amount_db(amount);
            assert!((db - 20.0 * (1.0 + 29.0 * amount * amount).log10()).abs() < 1e-5);
            assert!((amount_from_db(db).unwrap() - amount).abs() < 2e-6);
        }
        assert!((amount_from_db(6.0).unwrap() - 0.185_255).abs() < 1e-5);
        for invalid in [-0.1, 30.0, f32::NAN, f32::INFINITY] {
            assert!(amount_from_db(invalid).is_none());
        }
    }

    #[test]
    fn displayed_values_parse_back_to_exact_current_sound() {
        for kind in [DriveValue::Amount, DriveValue::Output, DriveValue::Percent] {
            for current in [0.0, 0.001_246, 0.185_255, 0.35, 0.5, 0.973_213, 1.0] {
                let shown = kind.display(current);
                assert_eq!(kind.parse(&shown, current), Some(current), "{kind:?} {shown}");
            }
        }
        assert_eq!(DriveValue::Output.display(0.0), "-inf");
        assert_eq!(DriveValue::Output.parse("-inf dB", 1.0), Some(0.0));
        assert_eq!(DriveValue::Output.parse("-∞", 1.0), Some(0.0));
        assert_eq!(DriveValue::Output.parse("mute", 1.0), Some(0.0));
        assert!((DriveValue::Output.parse("-6 dB", 1.0).unwrap() - 0.501_187_2).abs() < 1e-6);
        assert_eq!(DriveValue::Percent.parse("80 %", 0.5), Some(0.8));
        for kind in [DriveValue::Amount, DriveValue::Output, DriveValue::Percent] {
            for invalid in ["", "NaN", "inf", "oops", "1000", "-1000"] {
                assert!(kind.parse(invalid, 0.5).is_none(), "{kind:?} {invalid}");
            }
        }
        assert_eq!(DriveValue::Amount.parse("29.54 dB", 0.35), Some(1.0));
        assert_eq!(DriveValue::Output.parse("6.02 dB", 1.0), Some(2.0));
    }

    #[test]
    fn amount_knob_moves_in_pre_gain_decibels_and_output_in_linear_gain() {
        for position in [0.0, 0.1, 0.25, 0.5, 0.75, 1.0] {
            let amount = DriveValue::Amount.value(position);
            assert!((DriveValue::Amount.position(amount) - position).abs() < 1e-6);
            assert!((amount_db(amount) - position * amount_db(1.0)).abs() < 1e-5);
            let output = DriveValue::Output.value(position);
            assert!((DriveValue::Output.position(output) - position).abs() < 1e-6);
        }
        assert_eq!(DriveValue::Output.value(0.0), 0.0);
        assert_eq!(DriveValue::Output.value(0.5), 1.0);
        assert_eq!(DriveValue::Output.value(1.0), 2.0);
    }
}
