/// Metering values for a block of audio.
///
/// All values are non-negative and represent absolute amplitude.
/// Peak values are the maximum absolute sample value per channel.
/// RMS values are the root-mean-square amplitude per channel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeterValues {
    pub peak_l: f32,
    pub peak_r: f32,
    pub rms_l: f32,
    pub rms_r: f32,
}

impl MeterValues {
    /// A silent meter reading.
    pub const SILENT: Self = Self {
        peak_l: 0.0,
        peak_r: 0.0,
        rms_l: 0.0,
        rms_r: 0.0,
    };
}

impl Default for MeterValues {
    fn default() -> Self {
        Self::SILENT
    }
}

/// Calculate peak and RMS meter values from interleaved audio data.
///
/// `data` contains interleaved samples: `[L0, R0, L1, R1, ...]` for stereo.
/// `channels` is the number of interleaved channels (typically 2).
///
/// For mono input (`channels == 1`), the same values are reported for both
/// left and right.  For multi-channel input (`channels > 2`), only the first
/// two channels are metered.
///
/// Returns `MeterValues::SILENT` if `data` is empty or `channels` is 0.
///
/// This function is allocation-free and suitable for use in the audio thread.
pub fn calculate_meters(data: &[f32], channels: usize) -> MeterValues {
    if data.is_empty() || channels == 0 {
        return MeterValues::SILENT;
    }

    let frames = data.len() / channels;
    if frames == 0 {
        return MeterValues::SILENT;
    }

    let mut peak_l: f32 = 0.0;
    let mut peak_r: f32 = 0.0;
    let mut sum_sq_l: f64 = 0.0;
    let mut sum_sq_r: f64 = 0.0;

    for frame in 0..frames {
        let base = frame * channels;

        // Left channel (channel 0)
        let l = data[base];
        let abs_l = l.abs();
        if abs_l > peak_l {
            peak_l = abs_l;
        }
        sum_sq_l += (l as f64) * (l as f64);

        // Right channel (channel 1, or channel 0 for mono)
        let r = if channels >= 2 { data[base + 1] } else { l };
        let abs_r = r.abs();
        if abs_r > peak_r {
            peak_r = abs_r;
        }
        sum_sq_r += (r as f64) * (r as f64);
    }

    let rms_l = (sum_sq_l / frames as f64).sqrt() as f32;
    let rms_r = (sum_sq_r / frames as f64).sqrt() as f32;

    MeterValues {
        peak_l,
        peak_r,
        rms_l,
        rms_r,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1e-6;

    #[test]
    fn silent_buffer() {
        let data = vec![0.0f32; 256];
        let m = calculate_meters(&data, 2);
        assert_eq!(m, MeterValues::SILENT);
    }

    #[test]
    fn empty_buffer() {
        let m = calculate_meters(&[], 2);
        assert_eq!(m, MeterValues::SILENT);
    }

    #[test]
    fn zero_channels() {
        let m = calculate_meters(&[1.0, 2.0], 0);
        assert_eq!(m, MeterValues::SILENT);
    }

    #[test]
    fn constant_amplitude_stereo() {
        // 4 frames of stereo: L=0.5, R=-0.25
        let data: Vec<f32> = (0..4).flat_map(|_| vec![0.5f32, -0.25f32]).collect();
        let m = calculate_meters(&data, 2);

        assert!((m.peak_l - 0.5).abs() < EPSILON);
        assert!((m.peak_r - 0.25).abs() < EPSILON);
        // RMS of constant signal = absolute value of that signal
        assert!((m.rms_l - 0.5).abs() < EPSILON);
        assert!((m.rms_r - 0.25).abs() < EPSILON);
    }

    #[test]
    fn peak_detects_maximum() {
        // Stereo: L channel has a spike, R channel is quiet
        let mut data = vec![0.0f32; 20]; // 10 frames stereo
        data[4 * 2] = 0.9; // frame 4, left channel
        data[7 * 2 + 1] = -0.7; // frame 7, right channel

        let m = calculate_meters(&data, 2);
        assert!((m.peak_l - 0.9).abs() < EPSILON);
        assert!((m.peak_r - 0.7).abs() < EPSILON);
    }

    #[test]
    fn rms_of_sine_wave() {
        // Generate a full cycle of a sine wave at both channels.
        let frames = 4096;
        let mut data = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let s = (2.0 * std::f32::consts::PI * i as f32 / frames as f32).sin();
            data.push(s); // L
            data.push(s); // R
        }
        let m = calculate_meters(&data, 2);

        // RMS of a sine wave = 1/sqrt(2) ~ 0.7071
        let expected_rms = 1.0_f32 / 2.0_f32.sqrt();
        assert!((m.rms_l - expected_rms).abs() < 0.01);
        assert!((m.rms_r - expected_rms).abs() < 0.01);
        assert!((m.peak_l - 1.0).abs() < 0.01);
        assert!((m.peak_r - 1.0).abs() < 0.01);
    }

    #[test]
    fn mono_duplicates_to_both_channels() {
        let data = vec![0.5f32, -0.8, 0.3, 0.6];
        let m = calculate_meters(&data, 1);

        assert!((m.peak_l - 0.8).abs() < EPSILON);
        assert!((m.peak_r - 0.8).abs() < EPSILON);
        assert!((m.rms_l - m.rms_r).abs() < EPSILON);
    }

    #[test]
    fn negative_values_produce_positive_peak() {
        let data = vec![-0.9f32, -0.7, -0.3, -0.1]; // 2 frames stereo
        let m = calculate_meters(&data, 2);

        assert!((m.peak_l - 0.9).abs() < EPSILON);
        assert!((m.peak_r - 0.7).abs() < EPSILON);
    }

    #[test]
    fn meter_values_default_is_silent() {
        assert_eq!(MeterValues::default(), MeterValues::SILENT);
    }

    #[test]
    fn single_frame_stereo() {
        let data = vec![0.3f32, 0.6];
        let m = calculate_meters(&data, 2);
        assert!((m.peak_l - 0.3).abs() < EPSILON);
        assert!((m.peak_r - 0.6).abs() < EPSILON);
        assert!((m.rms_l - 0.3).abs() < EPSILON);
        assert!((m.rms_r - 0.6).abs() < EPSILON);
    }

    #[test]
    fn multichannel_only_meters_first_two() {
        // 4-channel data: only channels 0 and 1 should be metered
        let data = vec![
            0.1f32, 0.2, 0.9, 0.9, // frame 0
            0.3, 0.4, 0.9, 0.9, // frame 1
        ];
        let m = calculate_meters(&data, 4);
        assert!((m.peak_l - 0.3).abs() < EPSILON);
        assert!((m.peak_r - 0.4).abs() < EPSILON);
    }
}
