//! Native Drive presentation preserves the original stored nonlinear amount and linear gain.

pub fn amount_db(amount: f32) -> f32 {
    20.0 * (1.0 + 29.0 * amount * amount).log10()
}

pub fn amount_from_db(db: f32) -> Option<f32> {
    (db.is_finite() && (0.0..=amount_db(1.0)).contains(&db))
        .then(|| ((10.0_f32.powf(db / 20.0) - 1.0) / 29.0).max(0.0).sqrt())
}

#[derive(Clone, Copy, Debug)]
pub enum DriveValue {
    Amount,
    Output,
    Percent,
}

impl DriveValue {
    pub fn display(self, value: f32) -> String {
        match self {
            Self::Amount => format!("{:.2}", amount_db(value)),
            Self::Output if value == 0.0 => "-inf".into(),
            Self::Output => format!("{:.2}", 20.0 * value.log10()),
            Self::Percent => format!("{:.0}", 100.0 * value),
        }
    }

    pub fn parse(self, text: &str, current: f32) -> Option<f32> {
        let text = text.trim().to_ascii_lowercase();
        let text = text.trim_end_matches(if matches!(self, Self::Percent) { "%" } else { "db" }).trim();
        if matches!(self, Self::Output) && matches!(text, "-inf" | "-∞" | "mute") {
            return Some(0.0);
        }
        let number = text.parse::<f32>().ok()?;
        if !number.is_finite() {
            return None;
        }
        // A rounded readout describes its exact current sound until the user changes it.
        if self.display(current).parse::<f32>().ok() == Some(number) {
            return Some(current);
        }
        match self {
            Self::Amount if text == self.display(1.0) => Some(1.0),
            Self::Amount => amount_from_db(number),
            Self::Output if text == self.display(2.0) => Some(2.0),
            Self::Output => {
                let amplitude = 10.0_f32.powf(number / 20.0);
                (amplitude > 0.0 && amplitude <= 2.0).then_some(amplitude)
            }
            Self::Percent => (0.0..=100.0).contains(&number).then_some(number / 100.0),
        }
    }


}

pub fn knob_position(amount: f32) -> f32 {
    (amount_db(amount) / amount_db(1.0)).clamp(0.0, 1.0)
}

pub fn knob_value(position: f32) -> f32 {
    amount_from_db(position.clamp(0.0, 1.0) * amount_db(1.0)).unwrap_or(1.0)
}

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
    fn amount_knob_moves_in_pre_gain_decibels() {
        for position in [0.0, 0.1, 0.25, 0.5, 0.75, 1.0] {
            let amount = knob_value(position);
            assert!((knob_position(amount) - position).abs() < 1e-6);
            assert!((amount_db(amount) - position * amount_db(1.0)).abs() < 1e-5);

        }
    }
}
