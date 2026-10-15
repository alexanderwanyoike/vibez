//! Retains fresh native Delay observations independently of device disclosure and editing.

use std::time::{Duration, Instant};
use vibez_core::{delay_state::DelayState, effect::EffectType};
use vibez_dsp::delay::{param, DELAY_PARAMS};
use crate::state::UiEffect;
use super::delay_layout::Page;

#[derive(Default)]
pub struct DelayObservation {
    pub snapshot: Option<DelayState>,
    pub page: Page,
    last_at: Option<Instant>,
    last_order: u64,
}

pub fn is_native_delay(effect: &UiEffect) -> bool {
    effect.effect_type == EffectType::Delay && effect.plugin_ref.is_none() && effect.plugin_name.is_none()
}

fn valid(state: &DelayState, rate: u32) -> bool {
    if state.sample_rate != rate || !(1000..=466033).contains(&rate)
        || state.processed_frames == 0 || state.observation_order == 0
        || !state.bpm.is_finite() || !(20.0..=999.0).contains(&state.bpm)
        || !state.mix.is_finite() || !(0.0..=1.0).contains(&state.mix)
        || !state.feedback.is_finite() || !(0.0..=0.95).contains(&state.feedback)
        || !state.routing.is_finite() || !(0.0..=1.0).contains(&state.routing)
        || !state.duck_attenuation_db.is_finite() || !(0.0..=24.0).contains(&state.duck_attenuation_db)
    { return false; }
    for (index, (&value, descriptor)) in state.targets.iter().zip(DELAY_PARAMS).enumerate() {
        if !value.is_finite() || !(descriptor.min..=descriptor.max).contains(&value)
            || (matches!(index, param::SYNC | param::LEFT_DIVISION | param::LEFT_MODIFIER |
                param::ROUTING | param::LINK | param::RIGHT_DIVISION | param::RIGHT_MODIFIER |
                param::FEEDBACK_HP_ENABLED | param::FEEDBACK_LP_ENABLED | param::TIME_CHANGE)
                && value.fract() != 0.0) { return false; }
    }
    let maximum = 18.0 * f64::from(rate);
    state.lines.iter().all(|line| {
        [line.old_samples, line.destination_samples, line.target_samples].into_iter()
            .chain(line.pending_samples).all(|value| value.is_finite() && (1.0..=maximum).contains(&value))
            && line.progress.is_finite() && (0.0..=1.0).contains(&line.progress)
            && line.moving == (line.remaining_frames > 0) && (!line.gliding || line.moving)
    }) && state.filters.iter().all(|filter| filter.enabled.is_finite()
        && (0.0..=1.0).contains(&filter.enabled) && filter.cutoff_hz.is_finite()
        && filter.cutoff_hz > 0.0 && filter.cutoff_hz <= 0.45 * f64::from(rate))
}

impl DelayObservation {
    pub fn accept(&mut self, snapshot: Option<DelayState>, rate: u32, now: Instant) {
        let Some(snapshot) = snapshot else { self.clear(); return; };
        if !valid(&snapshot, rate) { self.clear(); return; }
        if snapshot.observation_order <= self.last_order { return; }
        self.last_order = snapshot.observation_order;
        self.last_at = Some(now);
        self.snapshot = Some(snapshot);
    }

    pub fn clear(&mut self) {
        self.snapshot = None;
        self.last_at = None;
    }

    pub fn restart(&mut self) {
        self.clear();
        self.last_order = 0;
    }

    pub fn expire(&mut self, now: Instant) {
        if self.last_at.is_some_and(|last| now.saturating_duration_since(last) >= Duration::from_millis(250)) {
            self.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_dsp::{delay::DelayEffect, effect::AudioEffect};

    #[test]
    fn fresh_silence_is_valid_while_old_malformed_and_expired_state_cannot_replace_it() {
        let mut effect = DelayEffect::new(1000.0);
        effect.process(&mut [0.0; 20], 2);
        let first = effect.delay_snapshot().unwrap();
        let now = Instant::now();
        let mut panel = DelayObservation { page: Page::Right, ..Default::default() };
        panel.accept(Some(first), 1000, now);
        assert_eq!(panel.snapshot, Some(first));
        effect.process(&mut [0.0; 2], 2);
        let newer = effect.delay_snapshot().unwrap();
        panel.accept(Some(newer), 1000, now);
        panel.accept(Some(first), 1000, now);
        assert_eq!(panel.snapshot, Some(newer));
        panel.expire(now + Duration::from_millis(249));
        assert!(panel.snapshot.is_some());
        panel.expire(now + Duration::from_millis(250));
        assert!(panel.snapshot.is_none());
        assert_eq!(panel.page, Page::Right);
        panel.restart();
        panel.accept(Some(first), 1000, now);
        assert!(panel.snapshot.is_some());
        let mut malformed = newer;
        malformed.lines[0].progress = f64::NAN;
        panel.accept(Some(malformed), 1000, now);
        assert!(panel.snapshot.is_none());
        panel.restart();
        panel.accept(Some(first), 48000, now);
        assert!(panel.snapshot.is_none());
        panel.accept(Some(first), 1000, now);
        panel.accept(None, 1000, now);
        assert!(panel.snapshot.is_none());
        assert_eq!(panel.page, Page::Right);
    }
}
