//! Routes generation-checked native Bitcrush packets within the selected prepared owner lifecycle.

use super::*;
use crate::domains::bitcrush::is_native_bitcrush;
use vibez_core::{
    effect::SampleHoldMeter,
    id::{EffectId, TrackId},
};

impl App {
    pub(super) fn bitcrush_sample_hold(&self, effect: EffectId) -> Option<SampleHoldMeter> {
        self.bitcrush_panels.get(&effect)?.meter
    }

    pub(super) fn poll_bitcrush_panels(
        &mut self,
        selected: Option<TrackId>,
        now: std::time::Instant,
    ) {
        let project = &self.state.project_tracks;
        self.bitcrush_panels.retain(|id, _| {
            project
                .tracks
                .iter()
                .chain(&project.buses)
                .chain(std::iter::once(&project.master))
                .any(|track| {
                    track
                        .effects
                        .iter()
                        .any(|effect| effect.id == *id && is_native_bitcrush(effect))
                })
        });
        for track in project
            .tracks
            .iter()
            .chain(&project.buses)
            .chain(std::iter::once(&project.master))
        {
            for effect in track.effects.iter().filter(|effect| is_native_bitcrush(effect)) {
                let Some(panel) = self.bitcrush_panels.get_mut(&effect.id) else {
                    continue;
                };
                let prepared_rate = self.cmd_tx.prepared_effect_sample_rate(track.id, effect.id);
                if selected != Some(track.id)
                    || effect.bypass
                    || effect.reconfiguration_failed
                    || prepared_rate.is_none()
                    || panel.meter.is_some_and(|meter| {
                        Some(meter.sample_rate) != prepared_rate.map(|rate| rate as f32)
                    })
                {
                    panel.reset_metering();
                } else {
                    panel.reset_stale(now);
                }
            }
        }
    }

    pub(super) fn accept_bitcrush_sample_hold(
        &mut self,
        track: TrackId,
        effect: EffectId,
        generation: u64,
        meter: SampleHoldMeter,
    ) {
        if self.device_meters.channel != Some(track) || self.device_meters.generation != generation
        {
            return;
        }
        let Some(device) = self.state.find_track(track).and_then(|track| {
            track
                .effects
                .iter()
                .find(|candidate| candidate.id == effect && is_native_bitcrush(candidate))
        }) else {
            return;
        };
        let prepared_rate = self.cmd_tx.prepared_effect_sample_rate(track, effect);
        let panel = self.bitcrush_panels.entry(effect).or_default();
        if device.bypass
            || device.reconfiguration_failed
            || prepared_rate.is_none()
            || Some(meter.sample_rate) != prepared_rate.map(|rate| rate as f32)
        {
            panel.reset_metering();
        } else {
            panel.accept_meter(meter, std::time::Instant::now());
        }
    }
}
