//! Gain control values and transient metering state, independent of rendering.

pub fn parse_input(_index: usize, _text: &str) -> Option<f32> {
    None
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
}
