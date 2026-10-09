//! Delayed musical coordinates and automation values share the source clock.

use vibez_core::automation::{AutomationLane, AutomationTarget};
use vibez_core::id::TrackId;
use vibez_dsp::compensation_delay::CompensationDelay;

#[derive(Debug)]
pub struct ChannelClock {
    pub track: TrackId,
    positions: Vec<u64>,
    written: u64,
    block_start: u64,
    history_start: u64,
}

impl ChannelClock {
    pub fn prepare(track: TrackId, max_delay: u32, frames: usize) -> Result<Self, String> {
        let count = (max_delay as usize)
            .checked_add(frames)
            .and_then(|count| count.checked_add(1))
            .ok_or("Channel clock size overflow")?;
        let mut positions = Vec::new();
        positions
            .try_reserve_exact(count)
            .map_err(|_| "Unable to allocate channel clock history")?;
        positions.resize(count, 0);
        Ok(Self {
            track,
            positions,
            written: 0,
            block_start: 0,
            history_start: 0,
        })
    }
    pub fn storage_bytes(&self) -> usize {
        self.positions.len() * std::mem::size_of::<u64>()
    }
    pub fn clear(&mut self) {
        self.written = 0;
        self.block_start = 0;
    }
    pub fn record(&mut self, position: u64, frames: usize, advancing: bool) {
        self.block_start = self.written;
        if self.written == 0 {
            self.history_start = position;
        }
        for offset in 0..frames {
            let index = (self.written % self.positions.len() as u64) as usize;
            self.positions[index] =
                position.saturating_add(if advancing { offset as u64 } else { 0 });
            self.written += 1;
        }
    }
    pub fn before_block(&self, delay: u32) -> Option<u64> {
        if delay == 0 || self.written < delay as u64 {
            return None;
        }
        let index = self.written - delay as u64;
        Some(self.positions[(index % self.positions.len() as u64) as usize])
    }
    pub fn has_context(&self, delay: u32, offset: usize) -> bool {
        self.block_start.saturating_add(offset as u64) >= delay as u64
    }
    pub fn position(&self, delay: u32, offset: usize) -> u64 {
        let source = self.block_start.saturating_add(offset as u64);
        if source < delay as u64 {
            // Later blocks and loop wraps cannot rebase the origin used before
            // the first delayed source sample becomes resident.
            return self.history_start.saturating_sub(delay as u64 - source);
        }
        let index = source - delay as u64;
        self.positions[(index % self.positions.len() as u64) as usize]
    }
}

#[derive(Debug)]
pub struct PreparedAutomationControl {
    pub track: TrackId,
    pub target: AutomationTarget,
    pub node: usize,
    delay: CompensationDelay,
    pub values: Vec<f32>,
}

impl PreparedAutomationControl {
    pub fn prepare(
        track: TrackId,
        target: AutomationTarget,
        node: usize,
        delay: u32,
        frames: usize,
        budget: usize,
    ) -> Result<Self, String> {
        let mut line = CompensationDelay::prepare(delay, 1, budget)
            .map_err(|error| format!("Automation history preparation failed: {error:?}"))?;
        line.fill_history(f32::NAN);
        Ok(Self {
            track,
            target,
            node,
            delay: line,
            values: vec![f32::NAN; frames],
        })
    }
    pub fn storage_samples(&self) -> usize {
        self.delay.storage_samples() + self.values.len()
    }
    pub fn clear(&mut self) {
        self.delay.fill_history(f32::NAN);
    }
    pub fn render(
        &mut self,
        lanes: &[AutomationLane],
        position: u64,
        frames: usize,
        samples_per_beat: f64,
        advancing: bool,
    ) {
        let lane = lanes.iter().find(|lane| lane.target == self.target);
        for frame in 0..frames {
            let musical = position.saturating_add(if advancing { frame as u64 } else { 0 });
            self.values[frame] = lane
                .and_then(|lane| lane.value_at(musical as f64 / samples_per_beat))
                .unwrap_or(f32::NAN);
        }
        self.delay.process(&mut self.values[..frames]);
    }
}

#[cfg(test)]
#[path = "compensation_controls_tests.rs"]
mod tests;
