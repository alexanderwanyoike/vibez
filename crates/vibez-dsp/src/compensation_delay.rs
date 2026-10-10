//! Sample-exact storage for artificial processing-delay alignment.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelayPreparationError {
    UnsupportedChannels,
    InvalidRetention,
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
    delay_frames: u32,
    transition_from: u32,
    transition_remaining: u32,
}

impl CompensationDelay {
    pub fn prepare(
        frames: u32,
        channels: usize,
        available_samples: usize,
    ) -> Result<Self, DelayPreparationError> {
        Self::prepare_retained(frames, frames, channels, available_samples)
    }

    pub fn prepare_retained(
        frames: u32,
        retained_frames: u32,
        channels: usize,
        available_samples: usize,
    ) -> Result<Self, DelayPreparationError> {
        let samples = Self::required_samples(retained_frames, channels)?;
        if frames > retained_frames {
            return Err(DelayPreparationError::InvalidRetention);
        }

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
            delay_frames: frames,
            transition_from: frames,
            transition_remaining: 0,
        })
    }

    pub fn required_samples(frames: u32, channels: usize) -> Result<usize, DelayPreparationError> {
        if !(1..=2).contains(&channels) {
            return Err(DelayPreparationError::UnsupportedChannels);
        }
        let samples = (frames as usize)
            .checked_mul(channels)
            .ok_or(DelayPreparationError::SizeOverflow)?;
        Ok(samples)
    }

    pub fn storage_samples(&self) -> usize {
        self.history.len()
    }

    pub fn frames(&self) -> usize {
        self.delay_frames as usize
    }

    /// A seek invalidates history even if the destination reuses the same plan.
    pub fn clear(&mut self) {
        self.history.fill(0.0);
        self.cursor = 0;
        self.transition_remaining = 0;
    }

    pub fn retain_from(&mut self, previous: &mut Self) {
        debug_assert_eq!(self.history.len(), previous.history.len());
        debug_assert_eq!(self.channels, previous.channels);
        std::mem::swap(&mut self.history, &mut previous.history);
        self.cursor = previous.cursor;
        self.transition_from = if previous.transition_remaining == 64 {
            previous.transition_from
        } else {
            previous.delay_frames
        };
        self.transition_remaining = if self.delay_frames == self.transition_from {
            0
        } else {
            64
        };
    }

    pub fn fill_history(&mut self, value: f32) {
        self.history.fill(value);
        self.cursor = 0;
    }

    pub fn process(&mut self, interleaved: &mut [f32]) {
        self.process_layout(interleaved, self.channels);
    }

    pub fn process_layout(&mut self, interleaved: &mut [f32], channels: usize) {
        debug_assert!(
            channels == self.channels || (channels == 1 && self.channels == 2),
            "Compensation delay received an unsupported processing layout"
        );
        if self.history.is_empty() {
            return;
        }
        for frame in interleaved.chunks_exact_mut(channels) {
            let mono = frame[0];
            let blend = 1.0 - self.transition_remaining as f32 / 64.0;
            for (channel, sample) in frame
                .iter_mut()
                .map(Some)
                .chain(std::iter::repeat_with(|| None))
                .take(self.channels)
                .enumerate()
            {
                let source = sample.as_deref().copied().unwrap_or(mono);
                let delayed = |delay: u32| {
                    if delay == 0 {
                        source
                    } else {
                        let index = (self.cursor + self.history.len()
                            - delay as usize * self.channels
                            + channel)
                            % self.history.len();
                        self.history[index]
                    }
                };
                let next = delayed(self.delay_frames);
                let output = if self.transition_remaining == 0 {
                    next
                } else {
                    delayed(self.transition_from) * (1.0 - blend) + next * blend
                };
                self.history[self.cursor + channel] = source;
                if let Some(sample) = sample {
                    *sample = output;
                }
            }
            self.cursor = (self.cursor + self.channels) % self.history.len();
            self.transition_remaining = self.transition_remaining.saturating_sub(1);
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
    #[test]
    fn mono_frames_in_stereo_storage_preserve_both_history_channels_across_segments() {
        for delay in [0, 3, 137] {
            let mut line = CompensationDelay::prepare(delay, 2, 274).unwrap();
            let mut input = [0.0; 180];
            input[0] = 1.0;
            for block in input.chunks_mut(7) {
                line.process_layout(block, 1);
            }
            assert_eq!(
                input.iter().position(|&sample| sample == 1.0),
                Some(delay as usize)
            );
            assert!(input
                .iter()
                .enumerate()
                .all(|(index, &sample)| index == delay as usize || sample == 0.0));
        }
        let mut line = CompensationDelay::prepare(3, 2, 6).unwrap();
        line.process_layout(&mut [1.0, 2.0], 1);
        let mut stereo = [0.0; 8];
        line.process_layout(&mut stereo, 2);
        assert_eq!(stereo, [0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 0.0, 0.0]);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "unsupported processing layout")]
    fn stereo_input_cannot_silently_bypass_a_mono_delay() {
        CompensationDelay::prepare(3, 1, 3)
            .unwrap()
            .process_layout(&mut [1.0; 8], 2);
    }
}

#[cfg(test)]
mod transition_tests {
    use super::*;

    #[test]
    fn invalid_retention_is_rejected_before_storage_or_processing() {
        assert!(matches!(
            CompensationDelay::prepare_retained(521, 137, 2, 1042),
            Err(DelayPreparationError::InvalidRetention)
        ));
    }

    #[test]
    fn zero_wait_retains_history_for_growing_and_shrinking_output_waits() {
        let mut current = CompensationDelay::prepare_retained(0, 521, 2, 1042).unwrap();
        let mut signal = [0.25; 2048];
        current.process(&mut signal);
        for wait in [521, 0, 521] {
            let mut next = CompensationDelay::prepare_retained(wait, 521, 2, 1042).unwrap();
            next.retain_from(&mut current);
            let mut block = [0.25; 1024];
            next.process(&mut block);
            assert!(block.iter().all(|&sample| sample == 0.25));
            current = next;
        }
    }

    #[test]
    fn wait_change_crossfades_old_and_new_taps_without_a_step() {
        let mut previous = CompensationDelay::prepare_retained(0, 137, 1, 137).unwrap();
        previous.process(&mut [0.0; 137]);
        let mut next = CompensationDelay::prepare_retained(137, 137, 1, 137).unwrap();
        next.retain_from(&mut previous);
        let mut output = [1.0; 64];
        next.process(&mut output);
        assert_eq!(output[0], 1.0);
        assert!(output
            .windows(2)
            .all(|pair| (pair[1] - pair[0]).abs() <= 1.0 / 64.0));
        assert_eq!(output[63], 1.0 / 64.0);
    }
}
