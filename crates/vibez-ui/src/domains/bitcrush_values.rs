//! Bitcrush clock edits express actual host-rate divisions without changing stored units.

pub fn parse_rate(text: &str, sample_rate: u32) -> Option<f32> {
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
    for divider in 1..32 {
        let boundary = (rate / f64::from(divider) + rate / f64::from(divider + 1)) * 0.5;
        if requested > boundary {
            return Some(divider as f32);
        }
    }
    Some(32.0)
}
