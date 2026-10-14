//! Routes native Compressor panel choices and generation-checked dynamics diagnostics.

use super::*;
use crate::domains::compressor::{is_native_compressor, CompressorMsg, CompressorPanel};
use vibez_core::{
    effect::DynamicsMeter,
    id::{EffectId, TrackId},
};

impl App {
    pub(super) fn update_compressor_control(&mut self, message: CompressorMsg) -> Task<Message> {
        let (track, effect) = message.destination();
        if self.state.find_track(track).is_some_and(|track| {
            track
                .effects
                .iter()
                .any(|candidate| candidate.id == effect && is_native_compressor(candidate))
        }) {
            self.compressor_panels
                .entry(effect)
                .or_default()
                .update(message);
        }
        Task::none()
    }
    pub(super) fn compressor_panel(&self, effect: EffectId) -> &CompressorPanel {
        self.compressor_panels
            .get(&effect)
            .unwrap_or(&self.empty_compressor_panel)
    }
    pub(super) fn poll_compressor_panels(
        &mut self,
        selected: Option<TrackId>,
        now: std::time::Instant,
    ) {
        let project = &self.state.project_tracks;
        self.compressor_panels.retain(|id, _| {
            project
                .tracks
                .iter()
                .chain(&project.buses)
                .chain(std::iter::once(&project.master))
                .any(|track| {
                    track
                        .effects
                        .iter()
                        .any(|effect| effect.id == *id && is_native_compressor(effect))
                })
        });
        if let Some(track) = selected.and_then(|track| self.state.find_track(track)) {
            for effect in track
                .effects
                .iter()
                .filter(|effect| is_native_compressor(effect))
            {
                let panel = self.compressor_panels.entry(effect.id).or_default();
                if effect.bypass {
                    panel.reset_metering();
                } else {
                    panel.reset_stale(now);
                }
            }
        }
    }
    pub(super) fn accept_compressor_dynamics(
        &mut self,
        track: TrackId,
        effect: EffectId,
        generation: u64,
        meter: DynamicsMeter,
    ) {
        if self.device_meters.channel != Some(track) || self.device_meters.generation != generation
        {
            return;
        }
        let Some(device) = self.state.find_track(track).and_then(|track| {
            track
                .effects
                .iter()
                .find(|candidate| candidate.id == effect && is_native_compressor(candidate))
        }) else {
            return;
        };
        let panel = self.compressor_panels.entry(effect).or_default();
        if device.bypass {
            panel.reset_metering();
        } else {
            panel.accept(meter, std::time::Instant::now());
        }
    }
}
