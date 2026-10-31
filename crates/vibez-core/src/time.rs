use serde::{Deserialize, Serialize};

/// Position in samples (absolute frame count).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
pub struct SampleTime(pub u64);

impl SampleTime {
    pub fn from_seconds(seconds: f64, sample_rate: u32) -> Self {
        Self((seconds * sample_rate as f64) as u64)
    }

    pub fn to_seconds(self, sample_rate: u32) -> f64 {
        self.0 as f64 / sample_rate as f64
    }
}

/// Every bar is 4/4 until the Project gains a time signature.
pub const BEATS_PER_BAR: f64 = 4.0;

/// Musical position in beats (quarter notes).
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Serialize, Deserialize)]
pub struct MusicalTime(pub f64);

impl MusicalTime {
    pub fn from_bars(bars: f64, beats_per_bar: f64) -> Self {
        Self(bars * beats_per_bar)
    }

    pub fn bar(self, beats_per_bar: f64) -> f64 {
        self.0 / beats_per_bar
    }

    /// A length as whole bars plus leftover beats, such as "1 bar 2 beats".
    pub fn length_label(self) -> String {
        let beats = (self.0.max(0.0) * 100.0).round() / 100.0;
        let bars = (beats / BEATS_PER_BAR).floor();
        let rest = beats - bars * BEATS_PER_BAR;
        let plural = |count: f64, unit: &str| {
            format!("{count} {unit}{}", if count == 1.0 { "" } else { "s" })
        };
        match (bars > 0.0, rest > 0.0) {
            (true, true) => format!("{} {}", plural(bars, "bar"), plural(rest, "beat")),
            (true, false) => plural(bars, "bar"),
            (false, _) => plural(rest, "beat"),
        }
    }
}

/// Converts between musical time and sample time at a fixed tempo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempoMap {
    pub bpm: f64,
    pub sample_rate: u32,
}

impl TempoMap {
    pub fn new(bpm: f64, sample_rate: u32) -> Self {
        Self { bpm, sample_rate }
    }

    /// Seconds per beat at the current tempo.
    pub fn seconds_per_beat(&self) -> f64 {
        60.0 / self.bpm
    }

    /// Samples per beat at the current tempo and sample rate.
    pub fn samples_per_beat(&self) -> f64 {
        self.seconds_per_beat() * self.sample_rate as f64
    }

    /// Convert musical time (beats) to sample time.
    pub fn musical_to_sample(&self, musical: MusicalTime) -> SampleTime {
        let samples = musical.0 * self.samples_per_beat();
        SampleTime(samples as u64)
    }

    /// Convert sample time to musical time (beats).
    pub fn sample_to_musical(&self, sample: SampleTime) -> MusicalTime {
        let beats = sample.0 as f64 / self.samples_per_beat();
        MusicalTime(beats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_time_seconds_roundtrip() {
        let sr = 44_100;
        let st = SampleTime::from_seconds(2.5, sr);
        let secs = st.to_seconds(sr);
        assert!((secs - 2.5).abs() < 1e-6);
    }

    #[test]
    fn musical_time_bars() {
        let mt = MusicalTime::from_bars(2.0, 4.0);
        assert!((mt.0 - 8.0).abs() < 1e-10);
        assert!((mt.bar(4.0) - 2.0).abs() < 1e-10);
    }

    #[test]
    fn length_label_reads_in_bars_and_leftover_beats() {
        let label = |beats| MusicalTime(beats).length_label();
        assert_eq!(label(16.0), "4 bars");
        assert_eq!(label(4.0), "1 bar");
        assert_eq!(label(6.0), "1 bar 2 beats");
        assert_eq!(label(1.0), "1 beat");
        assert_eq!(label(1.5), "1.5 beats");
        assert_eq!(label(12.25), "3 bars 0.25 beats");
    }

    #[test]
    fn length_label_hides_float_error_in_the_leftover_beats() {
        let label = |beats| MusicalTime(beats).length_label();
        assert_eq!(label(4.1), "1 bar 0.1 beats");
        assert_eq!(label(12.3), "3 bars 0.3 beats");
        assert_eq!(label(4.0 + 1.0 / 3.0), "1 bar 0.33 beats");
    }

    #[test]
    fn tempo_map_120bpm() {
        let tm = TempoMap::new(120.0, 44_100);
        assert!((tm.seconds_per_beat() - 0.5).abs() < 1e-10);
        assert!((tm.samples_per_beat() - 22_050.0).abs() < 1e-6);
    }

    #[test]
    fn tempo_map_roundtrip() {
        let tm = TempoMap::new(140.0, 48_000);
        let musical = MusicalTime(4.0);
        let sample = tm.musical_to_sample(musical);
        let back = tm.sample_to_musical(sample);
        // Tolerance accounts for u64 truncation in musical_to_sample
        let tolerance = 1.0 / tm.samples_per_beat();
        assert!((back.0 - musical.0).abs() < tolerance);
    }

    #[test]
    fn tempo_map_beat_one_at_120bpm() {
        let tm = TempoMap::new(120.0, 44_100);
        // 1 beat at 120 BPM = 0.5 seconds = 22050 samples
        let st = tm.musical_to_sample(MusicalTime(1.0));
        assert_eq!(st.0, 22_050);
    }
}
