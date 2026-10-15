//! Owns bounded stereo Delay storage, activation admission, fractional taps and ring lifecycle.

const MAX_HISTORY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SYNC_SECONDS: u64 = 18;
const INTERPOLATION_GUARDS: u64 = 2;

pub(super) struct DelayHistory {
    sample_rate: f32,
    configuration_valid: bool,
    buffer_l: Vec<f32>,
    buffer_r: Vec<f32>,
    write_pos: usize,
}

impl DelayHistory {
    pub(super) fn new(sample_rate: f32) -> Self {
        let histories = Self::prepare_histories(sample_rate);
        let configuration_valid = histories.is_some();
        let (buffer_l, buffer_r) = histories.unwrap_or_default();
        Self {
            sample_rate,
            configuration_valid,
            buffer_l,
            buffer_r,
            write_pos: 0,
        }
    }

    fn prepare_histories(sample_rate: f32) -> Option<(Vec<f32>, Vec<f32>)> {
        let frames = Self::requested_activation_rate(sample_rate)
            .filter(|rate| *rate >= 1000)
            .and_then(|rate| u64::from(rate).checked_mul(MAX_SYNC_SECONDS))
            .and_then(|frames| frames.checked_add(INTERPOLATION_GUARDS))?;
        let bytes = frames
            .checked_mul(2)?
            .checked_mul(std::mem::size_of::<f32>() as u64)?;
        if bytes > MAX_HISTORY_BYTES {
            return None;
        }
        let count = usize::try_from(frames).ok()?;
        let mut buffer_l = Vec::new();
        buffer_l.try_reserve_exact(count).ok()?;
        buffer_l.resize(count, 0.0);
        let mut buffer_r = Vec::new();
        buffer_r.try_reserve_exact(count).ok()?;
        buffer_r.resize(count, 0.0);
        Some((buffer_l, buffer_r))
    }

    fn requested_activation_rate(sample_rate: f32) -> Option<u32> {
        let rate = f64::from(sample_rate);
        if rate.is_finite() && rate.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(&rate) {
            Some(rate as u32)
        } else {
            None
        }
    }

    pub(super) fn activation_sample_rate(&self) -> Option<u32> {
        Self::requested_activation_rate(self.sample_rate)
    }

    pub(super) fn is_valid(&self) -> bool {
        self.configuration_valid
    }

    pub(super) fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub(super) fn tap(&self, samples: f64) -> (usize, f64) {
        // Full-ring distance reads the prior lap before the current frame overwrites it.
        let delay = samples.max(1.0).min(self.buffer_l.len().max(1) as f64);
        (delay.floor() as usize, delay.fract())
    }

    pub(super) fn read(&self, (whole, fraction): (usize, f64)) -> (f32, f32) {
        let buf_len = self.buffer_l.len();
        let read = |history: &[f32], position: usize| {
            let recent = history[position];
            // Integer taps bypass blend arithmetic to retain the legacy sample algebra.
            if fraction == 0.0 {
                recent
            } else {
                let older = history[(position + buf_len - 1) % buf_len];
                (f64::from(recent) * (1.0 - fraction) + f64::from(older) * fraction) as f32
            }
        };
        let read_pos = (self.write_pos + buf_len - whole) % buf_len;
        (read(&self.buffer_l, read_pos), read(&self.buffer_r, read_pos))
    }

    pub(super) fn write_and_advance(&mut self, left: f32, right: f32) {
        self.buffer_l[self.write_pos] = left;
        self.buffer_r[self.write_pos] = right;
        self.write_pos = (self.write_pos + 1) % self.buffer_l.len();
    }

    pub(super) fn reset(&mut self) {
        self.buffer_l.fill(0.0);
        self.buffer_r.fill(0.0);
        self.write_pos = 0;
    }

    #[cfg(test)]
    pub(super) fn write_position(&self) -> usize {
        self.write_pos
    }

    #[cfg(test)]
    pub(super) fn left_samples(&self) -> &[f32] {
        &self.buffer_l
    }
}
