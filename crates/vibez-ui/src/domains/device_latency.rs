//! Cached device latency readouts in the current project sample rate.

pub fn latency_label(samples: Option<u32>, sample_rate: u32) -> String {
    match samples.filter(|_| sample_rate > 0) {
        Some(samples) => format!(
            "{samples} samples / {:.2} ms",
            samples as f64 * 1000.0 / sample_rate as f64
        ),
        None => "Latency unavailable".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn latency_readout_uses_current_rate() {
        assert_eq!(latency_label(Some(480), 48000), "480 samples / 10.00 ms");
        assert_eq!(latency_label(Some(480), 96000), "480 samples / 5.00 ms");
        assert_eq!(latency_label(None, 48000), "Latency unavailable");
    }
}
