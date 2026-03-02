/// Audio buffer adapter for converting between interleaved and deinterleaved formats.
///
/// External plugins (CLAP, VST3) use per-channel buffers, while vibez uses interleaved
/// `[L0, R0, L1, R1, ...]` buffers internally.
pub struct AudioBufferAdapter {
    channels: usize,
    per_channel: Vec<Vec<f32>>,
}

impl AudioBufferAdapter {
    pub fn new(channels: usize, max_frames: usize) -> Self {
        let per_channel = (0..channels).map(|_| vec![0.0; max_frames]).collect();
        Self {
            channels,
            per_channel,
        }
    }

    /// Deinterleave from an interleaved buffer into per-channel buffers.
    pub fn deinterleave(&mut self, interleaved: &[f32], frames: usize) {
        for ch in 0..self.channels {
            let buf = &mut self.per_channel[ch];
            for f in 0..frames {
                buf[f] = interleaved[f * self.channels + ch];
            }
        }
    }

    /// Reinterleave from per-channel buffers back into an interleaved buffer.
    pub fn interleave(&self, interleaved: &mut [f32], frames: usize) {
        for ch in 0..self.channels {
            let buf = &self.per_channel[ch];
            for f in 0..frames {
                interleaved[f * self.channels + ch] = buf[f];
            }
        }
    }

    /// Get mutable references to per-channel buffers (for passing to plugin process calls).
    pub fn channel_buffers_mut(&mut self) -> &mut [Vec<f32>] {
        &mut self.per_channel
    }

    /// Get the number of channels.
    pub fn channels(&self) -> usize {
        self.channels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_interleave() {
        let mut adapter = AudioBufferAdapter::new(2, 4);
        let input = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        adapter.deinterleave(&input, 4);

        assert_eq!(&adapter.per_channel[0][..4], &[1.0, 3.0, 5.0, 7.0]);
        assert_eq!(&adapter.per_channel[1][..4], &[2.0, 4.0, 6.0, 8.0]);

        let mut output = [0.0f32; 8];
        adapter.interleave(&mut output, 4);
        assert_eq!(output, input);
    }
}
