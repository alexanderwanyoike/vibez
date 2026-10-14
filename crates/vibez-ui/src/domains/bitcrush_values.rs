//! Native Bitcrush value edits preserve legacy storage with stepped bits and host-rate hertz.

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
    Some(if rate >= 1000.0 {
        format!("{:.3}k", rate / 1000.0)
    } else {
        format!("{rate:.3}")
    })
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
    let divider = (1..32)
        .find(|divider| {
            let boundary = (rate / f64::from(*divider) + rate / f64::from(*divider + 1)) * 0.5;
            requested > boundary
        })
        .unwrap_or(32) as f32;
    Some(
        if (1.0..=32.0).contains(&current) && current.round() == divider {
            current
        } else {
            divider
        },
    )
}
