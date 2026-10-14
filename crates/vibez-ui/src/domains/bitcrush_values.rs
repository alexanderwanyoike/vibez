//! Bitcrush clock edits express actual host-rate divisions without changing stored units.

pub fn parse_rate(text: &str, sample_rate: u32, current: f32) -> Option<f32> {
    if sample_rate < 2 {
        return None;
    }
    let text = text.trim().to_ascii_lowercase();
    let text = text.strip_suffix("hz").unwrap_or(&text).trim();
    let (number, multiplier) = match text.strip_suffix('k') {
        Some(number) => (number, 1000.0),
        None => (text, 1.0),
    };
    let requested = number.trim().parse::<f64>().ok()? * multiplier;
    let rate = f64::from(sample_rate);
    if !requested.is_finite() || !(rate / 32.0..=rate).contains(&requested) {
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
