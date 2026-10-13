//! Gain control values and transient metering state, independent of rendering.

pub const MIN_GAIN_DB: f32 = -60.0;
pub const MAX_GAIN_DB: f32 = 6.020_6;

pub fn gain_to_db(gain: f32) -> f32 {
    if gain > 0.0 {
        20.0 * gain.log10()
    } else {
        f32::NEG_INFINITY
    }
}

pub fn db_to_gain(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

pub fn parse_input(index: usize, text: &str) -> Option<f32> {
    let text = text.trim();
    if index == 0 && (text.eq_ignore_ascii_case("-inf") || text == "-∞") {
        return Some(0.0);
    }
    let db = text.parse::<f32>().ok()?;
    if !db.is_finite() {
        return None;
    }
    match index {
        0 => (MIN_GAIN_DB..=MAX_GAIN_DB)
            .contains(&db)
            .then(|| db_to_gain(db)),
        1 | 2 => (-24.0..=24.0).contains(&db).then_some(db),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_decibel_entry_preserves_native_amplitude_and_rejects_invalid_values() {
        assert_eq!(parse_input(0, "0"), Some(1.0));
        assert!((parse_input(0, " -6.0 ").unwrap() - 0.501_187_2).abs() < 1e-6);
        assert_eq!(parse_input(0, "-inf"), Some(0.0));
        assert_eq!(parse_input(0, "-∞"), Some(0.0));
        assert!((parse_input(0, "-60").unwrap() - 0.001).abs() < 1e-6);
        assert!((parse_input(0, "6.0205999").unwrap() - 2.0).abs() < 1e-6);

        for invalid in ["", "NaN", "inf", "+inf", "100", "-61", "oops"] {
            assert_eq!(parse_input(0, invalid), None, "accepted {invalid}");
        }
    }

    #[test]
    fn trim_entry_preserves_decibels_and_rejects_values_outside_its_range() {
        for index in [1, 2] {
            for db in [-24.0, -6.0, 0.0, 6.0, 24.0] {
                assert_eq!(parse_input(index, &db.to_string()), Some(db));
            }
            for invalid in ["-inf", "-∞", "NaN", "inf", "-24.1", "24.1", "oops"] {
                assert_eq!(parse_input(index, invalid), None, "accepted {invalid}");
            }
        }
        assert_eq!(parse_input(3, "0"), None);
    }
}
