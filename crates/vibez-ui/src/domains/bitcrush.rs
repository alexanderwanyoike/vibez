//! Owns native Bitcrush identity and availability of the latest genuine sample-hold packet.

use std::time::{Duration, Instant};
use vibez_core::effect::{EffectType, SampleHoldMeter};

pub fn is_native_bitcrush(effect: &crate::state::UiEffect) -> bool {
    effect.effect_type == EffectType::Bitcrush
        && effect.plugin_ref.is_none()
        && effect.plugin_name.is_none()
}

#[derive(Default)]
pub struct BitcrushPanel {
    pub meter: Option<SampleHoldMeter>,
    last_meter_at: Option<Instant>,
}

impl BitcrushPanel {
    pub fn accept_meter(&mut self, meter: SampleHoldMeter, now: Instant) {
        let count = usize::from(meter.count);
        if count == 0
            || count > meter.input.len()
            || !meter.sample_rate.is_finite()
            || meter.sample_rate <= 0.0
            || !meter.input[..count]
                .iter()
                .chain(&meter.output[..count])
                .all(|value| value.is_finite())
        {
            self.reset_metering();
            return;
        }
        self.meter = Some(meter);
        self.last_meter_at = Some(now);
    }

    pub fn reset_metering(&mut self) {
        self.meter = None;
        self.last_meter_at = None;
    }

    pub fn reset_stale(&mut self, now: Instant) {
        if self
            .last_meter_at
            .is_some_and(|last| now.saturating_duration_since(last) >= Duration::from_millis(250))
        {
            self.reset_metering();
        }
    }
}
