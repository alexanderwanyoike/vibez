//! Offline rendering warms the same graph from the song origin, then selects
//! compensated output corresponding to the requested musical interval.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BounceTiming {
    pub render_start: u64,
    pub render_end: u64,
    pub write_start: u64,
    pub write_end: u64,
}

impl BounceTiming {
    pub fn prepare(range: (u64, u64), latency: u32) -> Result<Self, &'static str> {
        if range.1 < range.0 {
            return Err("Bounce range ends before it starts");
        }
        let write_start = range
            .0
            .checked_add(latency as u64)
            .ok_or("Bounce preparation time overflow")?;
        let write_end = range
            .1
            .checked_add(latency as u64)
            .ok_or("Bounce flush time overflow")?;
        Ok(Self {
            render_start: 0,
            render_end: write_end,
            write_start,
            write_end,
        })
    }

    pub fn block_selection(
        self,
        render_position: u64,
        frames: usize,
    ) -> Option<std::ops::Range<usize>> {
        let end = render_position.checked_add(frames as u64)?;
        let first = self.write_start.max(render_position);
        let last = self.write_end.min(end);
        if first >= last {
            return None;
        }
        Some((first - render_position) as usize..(last - render_position) as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_dsp::compensation_delay::CompensationDelay;

    #[test]
    fn crop_preserves_range_edges_and_intentional_silence_with_prior_context() {
        for latency in [0, 137, 521] {
            let timing = BounceTiming::prepare((200, 1000), latency).unwrap();
            let mut delay = CompensationDelay::prepare(latency, 1, 521).unwrap();
            let mut written = Vec::new();
            let mut position = 0;
            while position < timing.render_end {
                let frames = (timing.render_end - position).min(31) as usize;
                let mut block: Vec<_> = (0..frames)
                    .map(|offset| {
                        let musical = position + offset as u64;
                        if [0, 199, 200, 201, 999, 1000].contains(&musical) {
                            musical as f32 + 1.0
                        } else {
                            0.0
                        }
                    })
                    .collect();
                delay.process(&mut block);
                if let Some(selection) = timing.block_selection(position, frames) {
                    written.extend_from_slice(&block[selection]);
                }
                position += frames as u64;
            }
            assert_eq!(written.len(), 800);
            assert_eq!(written[0], 201.0);
            assert_eq!(written[1], 202.0);
            assert_eq!(written[799], 1000.0);
            assert!(written[2..799].iter().all(|&sample| sample == 0.0));
            assert_eq!(timing.render_start, 0);
        }
    }

    #[test]
    fn impossible_windows_fail_before_rendering() {
        assert!(BounceTiming::prepare((1000, 200), 137).is_err());
        assert!(BounceTiming::prepare((0, u64::MAX), 137).is_err());
    }
}
