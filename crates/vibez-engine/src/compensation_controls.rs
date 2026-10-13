//! Delayed musical coordinates and automation values share the source clock.

use crate::guarded_history::GuardedHistory;
use vibez_core::automation::{AutomationLane, AutomationTarget};
use vibez_core::id::TrackId;
use vibez_dsp::compensation_delay::CompensationDelay;

#[derive(Debug)]
pub struct ChannelClock {
    pub track: TrackId,
    positions: GuardedHistory<u64>,
    block_start: u64,
    history_start: u64,
}

impl ChannelClock {
    pub fn prepare(track: TrackId, max_delay: u32, frames: usize) -> Result<Self, String> {
        if max_delay > crate::compensation::MAX_PATH_LATENCY {
            return Err("Channel clock delay exceeds the supported path budget".into());
        }
        let count =
            GuardedHistory::<u64>::required_count(max_delay, frames).map_err(str::to_owned)?;
        let positions = GuardedHistory::prepare(count).map_err(str::to_owned)?;
        Ok(Self {
            track,
            positions,
            block_start: 0,
            history_start: 0,
        })
    }
    pub fn storage_bytes(&self) -> usize {
        self.positions.storage_bytes()
    }
    pub fn clear(&mut self) {
        self.positions.clear();
        self.block_start = 0;
    }
    pub fn retain_from(&mut self, previous: &mut Self) {
        if self.positions.retain_from(&mut previous.positions) {
            self.block_start = previous.block_start;
            self.history_start = previous.history_start;
        }
    }
    pub fn record(&mut self, position: u64, frames: usize, advancing: bool) {
        self.block_start = self.positions.written();
        if self.positions.written() == 0 {
            self.history_start = position;
        }
        for offset in 0..frames {
            self.positions
                .push(position.saturating_add(if advancing { offset as u64 } else { 0 }));
        }
    }
    pub fn before_block(&self, delay: u32) -> Option<u64> {
        self.positions.before_block(delay)
    }
    pub fn has_context(&self, delay: u32, offset: usize) -> bool {
        self.block_start
            .saturating_add(offset as u64)
            .checked_sub(delay as u64)
            .and_then(|index| self.positions.get(index))
            .is_some()
    }
    pub fn segment_end(&self, delay: u32, offset: usize, limit: usize) -> usize {
        let valid = self.has_context(delay, offset);
        let start = self.position(delay, offset);
        let mut end = offset + 1;
        let step = if end < limit && self.position(delay, end) == start {
            0
        } else {
            1
        };
        while end < limit
            && self.has_context(delay, end) == valid
            && (!valid
                || self.position(delay, end)
                    == start.saturating_add(((end - offset) * step) as u64))
        {
            end += 1;
        }
        end
    }

    pub fn position(&self, delay: u32, offset: usize) -> u64 {
        // Empty command-drain callbacks have no current audio coordinate.
        if delay == 0 && offset == 0 && self.positions.written() == self.block_start {
            return 0;
        }
        let source = self.block_start.saturating_add(offset as u64);
        if source < delay as u64 {
            // Later blocks and loop wraps cannot rebase the origin used before
            // the first delayed source sample becomes resident.
            return self.history_start.saturating_sub(delay as u64 - source);
        }
        let index = source - delay as u64;
        let position = self.positions.get(index);
        debug_assert!(
            position.is_some(),
            "Channel clock exceeded its prepared retained history"
        );
        position.unwrap_or(self.history_start)
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
        if Self::required_samples(delay, frames)? > budget {
            return Err("Automation history exceeds the preparation storage budget".into());
        }
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
    pub fn required_samples(delay: u32, frames: usize) -> Result<usize, String> {
        let samples = CompensationDelay::required_samples(delay, 1)
            .map_err(|error| format!("Automation storage size failed: {error:?}"))?;
        samples
            .checked_add(frames)
            .ok_or_else(|| "Automation storage size overflow".into())
    }

    pub fn storage_samples(&self) -> usize {
        self.delay.storage_samples() + self.values.len()
    }
    pub fn clear(&mut self) {
        self.delay.fill_history(f32::NAN);
    }
    pub fn retain_from(&mut self, previous: &mut Self) {
        if self.delay.storage_samples() == previous.delay.storage_samples() {
            self.delay.retain_from(&mut previous.delay);
        } else {
            // Missing control history means no lane value, never numeric zero.
            self.clear();
        }
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

pub fn automation_stage(target: AutomationTarget) -> vibez_core::routing::NodeStage {
    use vibez_core::routing::NodeStage;
    match target {
        AutomationTarget::EffectParam { effect_id, .. }
        | AutomationTarget::PluginParam {
            effect_id: Some(effect_id),
            ..
        } => NodeStage::Effect(effect_id),
        AutomationTarget::InstrumentParam { .. }
        | AutomationTarget::PluginParam {
            effect_id: None, ..
        }
        | AutomationTarget::TrackSwingOffset => NodeStage::Source,
        AutomationTarget::TrackGain
        | AutomationTarget::TrackPan
        | AutomationTarget::TrackMute
        | AutomationTarget::Send { .. } => NodeStage::AfterFader,
    }
}
