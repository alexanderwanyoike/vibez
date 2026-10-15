//! Connects selected native Delay observations to persistent device panel state.

use super::*;
use crate::domains::delay_observation::is_native_delay;
use vibez_core::{delay_state::DelayState, id::{EffectId, TrackId}};

impl App {
    pub(super) fn accept_delay_state(&mut self, track_id: TrackId, effect_id: EffectId,
        generation: u64, snapshot: Option<DelayState>) {
        if self.device_meters.channel != Some(track_id) || self.device_meters.generation != generation { return; }
        let Some(effect) = self.state.find_track(track_id)
            .and_then(|track| track.effects.iter().find(|effect| effect.id == effect_id))
            .filter(|effect| is_native_delay(effect)) else { return; };
        let snapshot = if effect.bypass || effect.reconfiguration_failed { None } else { snapshot };
        self.delay_panels.entry(effect_id).or_default().accept(snapshot,
            self.state.transport.sample_rate, std::time::Instant::now());
    }

    pub(super) fn poll_delay_devices(&mut self) {
        let project = &self.state.project_tracks;
        self.delay_panels.retain(|id, _| project.tracks.iter().chain(&project.buses)
            .chain(std::iter::once(&project.master)).any(|track| track.effects.iter()
                .any(|effect| effect.id == *id && is_native_delay(effect))));
        let now = std::time::Instant::now();
        for panel in self.delay_panels.values_mut() { panel.expire(now); }
    }
}
