//! Conversions between linear amplitude and decibels.

/// Silence has no level in decibels: zero and negative amplitudes, and NaN,
/// are negative infinity.
pub fn amplitude_to_db(amplitude: f32) -> f32 {
    if amplitude > 0.0 {
        20.0 * amplitude.log10()
    } else {
        f32::NEG_INFINITY
    }
}

pub fn db_to_amplitude(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions_round_trip_and_silence_is_negative_infinity() {
        for db in [-60.0, -6.0, 0.0, 6.0] {
            assert!((amplitude_to_db(db_to_amplitude(db)) - db).abs() < 1e-4);
        }
        assert_eq!(amplitude_to_db(0.0), f32::NEG_INFINITY);
        assert_eq!(amplitude_to_db(-1.0), f32::NEG_INFINITY);
        assert_eq!(amplitude_to_db(f32::NAN), f32::NEG_INFINITY);
    }
}
