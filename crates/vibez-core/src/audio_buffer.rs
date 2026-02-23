/// Interleaved stereo audio buffer (used for engine I/O).
#[derive(Debug, Clone)]
pub struct AudioBuffer {
    /// Interleaved samples: [L0, R0, L1, R1, ...]
    pub data: Vec<f32>,
    pub channels: usize,
}

impl AudioBuffer {
    pub fn new(channels: usize, frames: usize) -> Self {
        Self {
            data: vec![0.0; channels * frames],
            channels,
        }
    }

    pub fn frames(&self) -> usize {
        if self.channels == 0 {
            return 0;
        }
        self.data.len() / self.channels
    }

    pub fn clear(&mut self) {
        self.data.fill(0.0);
    }

    pub fn sample(&self, frame: usize, channel: usize) -> f32 {
        self.data[frame * self.channels + channel]
    }

    pub fn set_sample(&mut self, frame: usize, channel: usize, value: f32) {
        self.data[frame * self.channels + channel] = value;
    }
}

/// Decoded audio data shared immutably between threads via Arc.
#[derive(Debug, Clone)]
pub struct DecodedAudio {
    /// Per-channel sample data: channels[ch][sample_idx]
    pub channels: Vec<Vec<f32>>,
    pub sample_rate: u32,
}

impl DecodedAudio {
    pub fn num_channels(&self) -> usize {
        self.channels.len()
    }

    pub fn num_frames(&self) -> usize {
        self.channels.first().map_or(0, |c| c.len())
    }

    pub fn duration_seconds(&self) -> f64 {
        self.num_frames() as f64 / self.sample_rate as f64
    }

    /// Get a sample, returning 0.0 if out of bounds.
    pub fn sample(&self, channel: usize, frame: usize) -> f32 {
        self.channels
            .get(channel)
            .and_then(|ch| ch.get(frame).copied())
            .unwrap_or(0.0)
    }

    /// Get peak amplitude for a range of frames (for waveform rendering).
    pub fn peak_in_range(&self, channel: usize, start: usize, end: usize) -> (f32, f32) {
        let ch = match self.channels.get(channel) {
            Some(ch) => ch,
            None => return (0.0, 0.0),
        };
        let start = start.min(ch.len());
        let end = end.min(ch.len());
        if start >= end {
            return (0.0, 0.0);
        }
        let mut min = f32::MAX;
        let mut max = f32::MIN;
        for &s in &ch[start..end] {
            if s < min {
                min = s;
            }
            if s > max {
                max = s;
            }
        }
        (min, max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_buffer_basics() {
        let mut buf = AudioBuffer::new(2, 128);
        assert_eq!(buf.frames(), 128);
        assert_eq!(buf.data.len(), 256);

        buf.set_sample(10, 0, 0.5);
        buf.set_sample(10, 1, -0.3);
        assert!((buf.sample(10, 0) - 0.5).abs() < 1e-10);
        assert!((buf.sample(10, 1) - (-0.3)).abs() < 1e-10);

        buf.clear();
        assert_eq!(buf.sample(10, 0), 0.0);
    }

    #[test]
    fn decoded_audio_basics() {
        let audio = DecodedAudio {
            channels: vec![vec![0.0, 0.5, -0.5, 1.0], vec![0.0, -0.5, 0.5, -1.0]],
            sample_rate: 44_100,
        };
        assert_eq!(audio.num_channels(), 2);
        assert_eq!(audio.num_frames(), 4);
        assert!((audio.duration_seconds() - 4.0 / 44_100.0).abs() < 1e-10);
    }

    #[test]
    fn decoded_audio_sample_out_of_bounds() {
        let audio = DecodedAudio {
            channels: vec![vec![1.0]],
            sample_rate: 44_100,
        };
        assert_eq!(audio.sample(0, 0), 1.0);
        assert_eq!(audio.sample(0, 999), 0.0);
        assert_eq!(audio.sample(5, 0), 0.0);
    }

    #[test]
    fn decoded_audio_peak_range() {
        let audio = DecodedAudio {
            channels: vec![vec![0.0, 0.5, -0.8, 0.3, -0.1]],
            sample_rate: 44_100,
        };
        let (min, max) = audio.peak_in_range(0, 1, 4);
        assert!((min - (-0.8)).abs() < 1e-10);
        assert!((max - 0.5).abs() < 1e-10);
    }

    #[test]
    fn decoded_audio_peak_empty_range() {
        let audio = DecodedAudio {
            channels: vec![vec![1.0, 2.0]],
            sample_rate: 44_100,
        };
        assert_eq!(audio.peak_in_range(0, 5, 10), (0.0, 0.0));
        assert_eq!(audio.peak_in_range(3, 0, 1), (0.0, 0.0));
    }
}
