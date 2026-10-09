//! Sample-exact storage for artificial processing-delay alignment.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelayPreparationError {
    UnsupportedChannels,
    SizeOverflow,
    StorageBudget,
    Allocation,
}

/// Allocated by preparation, then exclusively owned by the rendering thread.
/// Frame-major storage keeps stereo history coherent across segmented blocks.
#[derive(Debug)]
pub struct CompensationDelay {
    history: Vec<f32>,
    channels: usize,
    cursor: usize,
}

impl CompensationDelay {
    pub fn prepare(
        frames: u32,
        channels: usize,
        available_samples: usize,
    ) -> Result<Self, DelayPreparationError> {
        if !(1..=2).contains(&channels) {
            return Err(DelayPreparationError::UnsupportedChannels);
        }
        let samples = (frames as usize)
            .checked_mul(channels)
            .ok_or(DelayPreparationError::SizeOverflow)?;
        if samples > available_samples {
            return Err(DelayPreparationError::StorageBudget);
        }
        let mut history = Vec::new();
        history
            .try_reserve_exact(samples)
            .map_err(|_| DelayPreparationError::Allocation)?;
        history.resize(samples, 0.0);
        Ok(Self {
            history,
            channels,
            cursor: 0,
        })
    }

    pub fn storage_samples(&self) -> usize {
        self.history.len()
    }

    pub fn frames(&self) -> usize {
        self.history.len() / self.channels
    }

    /// A seek invalidates history even if the destination reuses the same plan.
    pub fn clear(&mut self) {
        self.history.fill(0.0);
        self.cursor = 0;
    }

    pub fn fill_history(&mut self, value: f32) {
        self.history.fill(value);
        self.cursor = 0;
    }

    pub fn process(&mut self, interleaved: &mut [f32]) {
        debug_assert!(interleaved.len().is_multiple_of(self.channels));
        if self.history.is_empty() {
            return;
        }
        for sample in interleaved {
            std::mem::swap(sample, &mut self.history[self.cursor]);
            self.cursor += 1;
            if self.cursor == self.history.len() {
                self.cursor = 0;
            }
        }
    }

    pub fn process_layout(&mut self, interleaved: &mut [f32], channels: usize) {
        if channels == self.channels {
            self.process(interleaved);
        } else if channels == 1 && self.channels == 2 && !self.history.is_empty() {
            for sample in interleaved {
                let output = self.history[self.cursor];
                self.history[self.cursor] = *sample;
                self.history[self.cursor + 1] = *sample;
                *sample = output;
                self.cursor = (self.cursor + 2) % self.history.len();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delayed_pulses(delay: u32, channels: usize, blocks: &[usize]) -> Vec<f32> {
        let frames = 1600;
        let mut signal = vec![0.0; frames * channels];
        for onset in [0, 37, 512, 1023] {
            for channel in 0..channels {
                signal[onset * channels + channel] = (channel + 1) as f32;
            }
        }
        let mut line = CompensationDelay::prepare(delay, channels, 4096).unwrap();
        let mut frame = 0;
        let mut block = 0;
        while frame < frames {
            let end = (frame + blocks[block % blocks.len()]).min(frames);
            line.process(&mut signal[frame * channels..end * channels]);
            frame = end;
            block += 1;
        }
        signal
    }

    #[test]
    fn non_block_aligned_delays_are_exact_for_segmented_buffers() {
        for delay in [0, 137, 521] {
            for channels in [1, 2] {
                let actual = delayed_pulses(delay, channels, &[1, 17, 511, 3, 64]);
                for frame in 0..1600 {
                    let pulse = [0, 37, 512, 1023]
                        .iter()
                        .any(|&onset| frame == onset + delay as usize);
                    for channel in 0..channels {
                        assert_eq!(
                            actual[frame * channels + channel],
                            if pulse { (channel + 1) as f32 } else { 0.0 }
                        );
                    }
                }
                assert_eq!(actual, delayed_pulses(delay, channels, &[512]));
            }
        }
    }

    #[test]
    fn seek_discards_previous_position_audio() {
        let mut line = CompensationDelay::prepare(137, 2, 274).unwrap();
        let mut block = vec![1.0; 128];
        line.process(&mut block);
        line.clear();
        let mut after_seek = vec![0.0; 1024];
        line.process(&mut after_seek);
        assert!(after_seek.iter().all(|&sample| sample == 0.0));
    }

    #[test]
    fn preparation_never_clamps_excessive_delay() {
        assert!(matches!(
            CompensationDelay::prepare(521, 2, 1041),
            Err(DelayPreparationError::StorageBudget)
        ));
        assert!(matches!(
            CompensationDelay::prepare(1, 3, usize::MAX),
            Err(DelayPreparationError::UnsupportedChannels)
        ));
    }
}
