//! Native Bitcrush value presentation and edits preserve stored targets and prepared-rate context.

pub fn parse_bits(text: &str, current: f32) -> Option<f32> {
    let text = text.trim().to_ascii_lowercase();
    let number = text
        .strip_suffix("bits")
        .or_else(|| text.strip_suffix("bit"))
        .unwrap_or(&text)
        .trim()
        .parse::<f32>()
        .ok()?;
    let descriptor = &vibez_dsp::bitcrush::BITCRUSH_PARAMS[vibez_dsp::bitcrush::param::BITS];
    let range = descriptor.min..=descriptor.max;
    if !range.contains(&number) || number.fract() != 0.0 {
        return None;
    }
    Some(if range.contains(&current) && current.trunc() == number {
        current
    } else {
        number
    })
}

pub fn display_rate(sample_rate: u32, current: f32) -> Option<String> {
    if sample_rate < 2 || !(1.0..=32.0).contains(&current) {
        return None;
    }
    let rate = f64::from(sample_rate) / f64::from(current.round());
    Some(display_hertz(rate))
}

fn display_hertz(rate: f64) -> String {
    if rate >= 1000.0 {
        format!("{:.3}k", rate / 1000.0)
    } else {
        format!("{rate:.3}")
    }
}

pub fn parameter_explanation(index: usize, values: &[f32], prepared_rate: Option<u32>) -> Option<String> {
    use vibez_dsp::bitcrush::{param, BITCRUSH_PARAMS};

    match index {
        param::SR_REDUCE => Some("Nominal Rate is the prepared host sample rate divided by the target interval N1..32. Edits snap to supported rates. Jitter and clock transitions can vary actual intervals; Clock reports the most recently acquired interval at packet end.".into()),
        param::JITTER => {
            let divider = values.get(param::SR_REDUCE).copied()
                .unwrap_or(BITCRUSH_PARAMS[param::SR_REDUCE].default);
            Some(if (1.0..1.5).contains(&divider) {
                "Jitter is inactive at the N1 target: its whole-frame acquisition interval is one source frame. An ongoing clock transition can still contain previous intervals.".into()
            } else {
                "Varies each stereo acquisition interval by up to half this percentage, rounded to whole source frames. Nominal Rate is the requested clock; Clock reports the most recently acquired interval.".into()
            })
        }
        param::POST_LP_HZ => {
            let target = values.get(index).copied().unwrap_or(BITCRUSH_PARAMS[index].default);
            Some(match prepared_rate.filter(|rate| *rate >= 2 && target.is_finite()) {
                Some(rate) => {
                    let effective = f64::from(target).min(0.49 * f64::from(rate));
                    format!("Effective prepared target: {} Hz, capped at 0.49 x the prepared host sample rate. Your cutoff setting is preserved. This is the target when Post LP is enabled; the applied cutoff may still be ramping.", display_hertz(effective))
                }
                None => "Effective prepared target unavailable until this native device has a valid prepared host sample rate. Your cutoff setting remains editable; actual applied cutoff can still be ramping.".into(),
            })
        }
        _ => None,
    }
}

fn requested_hertz(text: &str) -> Option<f64> {
    let text = text.trim().to_ascii_lowercase();
    let text = text.strip_suffix("hz").unwrap_or(&text).trim();
    let (number, multiplier) = match text.strip_suffix('k') {
        Some(number) => (number, 1000.0),
        None => (text, 1.0),
    };
    Some(number.trim().parse::<f64>().ok()? * multiplier)
}

fn nearest_divider(sample_rate: f64, requested: f64) -> f32 {
    (1..32)
        .find(|divider| {
            let boundary = (sample_rate / f64::from(*divider)
                + sample_rate / f64::from(*divider + 1)) * 0.5;
            requested > boundary
        })
        .unwrap_or(32) as f32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BitcrushValue { Bits, Rate }

impl BitcrushValue {
    fn selected(self, value: f32) -> f32 {
        match self {
            Self::Bits => value.trunc().clamp(2.0, 16.0),
            Self::Rate => value.round().clamp(1.0, 32.0),
        }
    }

    fn preserve(self, selected: f32, original: f32) -> f32 {
        let valid = match self {
            Self::Bits => (2.0..=16.0).contains(&original),
            Self::Rate => (1.0..=32.0).contains(&original),
        };
        if valid && self.selected(original) == selected { original } else { selected }
    }

    pub fn position(self, value: f32) -> f32 {
        match self {
            Self::Bits => (self.selected(value) - 2.0) / 14.0,
            Self::Rate => ((32.0_f64 / f64::from(self.selected(value))).ln()
                / 32.0_f64.ln()) as f32,
        }
    }

    pub fn value(self, position: f32, original: f32) -> f32 {
        let position = position.clamp(0.0, 1.0);
        let selected = match self {
            Self::Bits => (2.0 + position * 14.0).round(),
            // The host rate cancels from adjacent-Hz boundaries; this is a dimensionless ratio.
            Self::Rate => nearest_divider(1.0, 32.0_f64.powf(f64::from(position)) / 32.0),
        };
        self.preserve(selected, original)
    }

    pub fn step(self, value: f32, steps: i32, original: f32) -> f32 {
        let selected = match self {
            Self::Bits => (self.selected(value) + steps as f32).clamp(2.0, 16.0),
            Self::Rate => (self.selected(value) - steps as f32).clamp(1.0, 32.0),
        };
        self.preserve(selected, original)
    }
}

pub fn parse_rate(text: &str, sample_rate: u32, current: f32) -> Option<f32> {
    if sample_rate < 2 {
        return None;
    }
    let requested = requested_hertz(text)?;
    let rate = f64::from(sample_rate);
    if !requested.is_finite() {
        return None;
    }
    if display_rate(sample_rate, current).and_then(|shown| requested_hertz(&shown)) == Some(requested) {
        return Some(current);
    }
    if !(rate / 32.0..=rate).contains(&requested) {
        return None;
    }
    let divider = nearest_divider(rate, requested);
    Some(
        if (1.0..=32.0).contains(&current) && current.round() == divider {
            current
        } else {
            divider
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{display_rate, parse_rate};

    #[test]
    fn all_supported_rate_readouts_roundtrip_exact_and_fractional_storage_across_host_rates() {
        for sample_rate in [44_100_u32, 48_000, 96_000] {
            for divider in 1..=32 {
                let hertz = f64::from(sample_rate) / f64::from(divider);
                let expected = format!("{:.3}k", hertz / 1000.0);
                let exact = divider as f32;
                for current in [exact, (exact - 0.49).max(1.0), (exact + 0.49).min(32.0)] {
                    let shown = display_rate(sample_rate, current).expect("valid prepared rate");
                    assert_eq!(shown, expected, "Fs={sample_rate}, stored={current}");
                    for text in [
                        shown.clone(),
                        format!(" {} kHz ", shown.trim_end_matches('k')),
                        format!("{hertz:.3} Hz"),
                    ] {
                        let restored = parse_rate(&text, sample_rate, current)
                            .unwrap_or_else(|| panic!("Fs={sample_rate}, stored={current}: {text}"));
                        assert_eq!(
                            restored.to_bits(),
                            current.to_bits(),
                            "Fs={sample_rate}, stored={current}: {text}"
                        );
                    }
                }
            }

            let minimum = f64::from(sample_rate) / 32.0;
            for text in [
                format!("{} Hz", minimum - 1.0),
                format!("{} Hz", sample_rate + 1),
                "NaN".into(),
                "inf".into(),
                "-1 Hz".into(),
            ] {
                assert_eq!(parse_rate(&text, sample_rate, 32.0), None, "{text}");
            }
            let midpoint = sample_rate * 3 / 4;
            assert_eq!(parse_rate(&format!("{midpoint} Hz"), sample_rate, 1.0), Some(2.0));
        }

        for (sample_rate, expected) in [
            (44_100, "11.025k"),
            (48_000, "12.000k"),
            (96_000, "24.000k"),
        ] {
            assert_eq!(display_rate(sample_rate, 4.49).as_deref(), Some(expected));
        }
        assert_eq!(display_rate(44_100, 32.0).as_deref(), Some("1.378k"));
        assert_eq!(display_rate(8_000, 32.0).as_deref(), Some("250.000"));
        assert_eq!(parse_rate("250.000 Hz", 8_000, 31.75), Some(31.75));
        assert_eq!(display_rate(32_000, 32.0).as_deref(), Some("1.000k"));
        for sample_rate in [0, 1] {
            assert_eq!(display_rate(sample_rate, 4.49), None);
            assert_eq!(parse_rate("12.000k", sample_rate, 4.49), None);
        }
        for invalid in [0.0, 33.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(display_rate(44_100, invalid), None);
            assert_eq!(parse_rate("1.378k", 44_100, invalid), None);
        }
    }
}
