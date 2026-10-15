//! Select one visible device chain and publish bounded, lossy insert readings.

use super::*;

/// The one channel whose device meters the UI is showing, under the
/// generation that lets it reject readings from an earlier selection.
#[derive(Clone, Copy, Default)]
pub(super) struct EffectMeterTarget {
    channel: Option<TrackId>,
    generation: u64,
}

impl EffectMeterTarget {
    pub(super) fn generation_for(self, channel: TrackId) -> Option<u64> {
        (self.channel == Some(channel)).then_some(self.generation)
    }
}

impl AudioEngine {
    pub(super) fn set_effect_meter_channel(&mut self, track_id: Option<TrackId>, generation: u64) {
        self.effect_meter = EffectMeterTarget {
            channel: track_id,
            generation,
        };
        self.effect_meter_frames = 0;
        if let Some(channel) = track_id.and_then(|id| self.channel_mut(id)) {
            for slot in &mut channel.effects {
                slot.levels = Default::default();
            }
        }
    }

    pub(super) fn publish_effect_meters(&mut self, frames: usize) {
        let Some(track_id) = self.effect_meter.channel else {
            return;
        };
        self.effect_meter_frames = self.effect_meter_frames.saturating_add(frames as u64);
        let interval = u64::from(self.sample_rate.div_ceil(50).max(1));
        if self.effect_meter_frames < interval {
            return;
        }
        let frames = std::mem::take(&mut self.effect_meter_frames);
        // Borrows only the channel lists, not all of `self` as `channel_mut`
        // would, so events can be pushed while the channel is held.
        let channel = if track_id.is_master() {
            Some(&mut self.master)
        } else {
            self.tracks
                .iter_mut()
                .chain(&mut self.buses)
                .find(|channel| channel.id == track_id)
        };
        let Some(channel) = channel else {
            return;
        };
        for slot in &mut channel.effects {
            if !slot.effect.supports_level_meter() {
                continue;
            }
            let (input, output) = slot.levels.finish(frames, self.effect_meter.generation);
            let _ = self.event_tx.push(EngineEvent::EffectMeter {
                track_id,
                effect_id: slot.id,
                generation: self.effect_meter.generation,
                input,
                output,
            });
            if slot.effect.effect_type() == vibez_core::effect::EffectType::Delay {
                let snapshot = if slot.bypass { None } else { slot.effect.delay_snapshot() };
                let _ = self.event_tx.push(EngineEvent::EffectDelayState {
                    track_id, effect_id: slot.id, generation: self.effect_meter.generation,
                    observation_token: slot.levels.observation_token, snapshot,
                });
            }
        }
    }
}
