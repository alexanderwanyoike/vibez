//! Owns native Drive pages, stored-value presentation and the latest genuine waveform snapshot.

use std::time::{Duration, Instant};
use vibez_core::{effect::{EffectType, WaveShaperMeter}, id::{EffectId, TrackId}};
use vibez_dsp::drive::param;
use super::drive_values::DriveValue;

pub fn is_native_drive(effect: &crate::state::UiEffect) -> bool {
    effect.effect_type == EffectType::Drive && effect.plugin_ref.is_none() && effect.plugin_name.is_none()
}

pub fn value_presentation(index: usize) -> Option<DriveValue> {
    match index {
        param::AMOUNT => Some(DriveValue::Amount),
        param::OUTPUT => Some(DriveValue::Output),
        param::TONE | param::MIX => Some(DriveValue::Percent),
        _ => None,
    }
}

pub fn fresh_sound() -> Vec<f32> {
    let mut values: Vec<_> = vibez_dsp::drive::DRIVE_PARAMS.iter().map(|descriptor| descriptor.default).collect();
    values[param::CHARACTER] = 1.0;
    values
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DrivePage {
    Sound,
    #[default]
    Filters,
    Quality,
    Metering,
}
impl DrivePage {
    pub const ALL: [Self; 4] = [Self::Sound, Self::Filters, Self::Quality, Self::Metering];
}
impl std::fmt::Display for DrivePage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self { Self::Sound => "Sound", Self::Filters => "Filters", Self::Quality => "Quality", Self::Metering => "Metering" })
    }
}

#[derive(Clone, Debug)]
pub enum DriveMsg {
    Page(TrackId, EffectId, DrivePage),
}
impl DriveMsg {
    pub fn destination(&self) -> (TrackId, EffectId) {
        match self { Self::Page(track, effect, _) => (*track, *effect) }
    }
}

#[derive(Default)]
pub struct DrivePanel {
    pub page: DrivePage,
    pub meter: Option<WaveShaperMeter>,
    last_meter_at: Option<Instant>,
}
impl DrivePanel {
    pub fn update(&mut self, message: DriveMsg) {
        match message { DriveMsg::Page(_, _, page) => self.page = page }
    }
    pub fn accept_meter(&mut self, meter: WaveShaperMeter, now: Instant) {
        let count = usize::from(meter.count);
        if count == 0 || count > meter.input.len() || !meter.sample_rate.is_finite() || meter.sample_rate <= 0.0
            || !meter.input[..count].iter().chain(&meter.output[..count]).chain(&meter.transfer).all(|value| value.is_finite())
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
        if self.last_meter_at.is_some_and(|last| now.saturating_duration_since(last) >= Duration::from_millis(250)) {
            self.reset_metering();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waveform_silence_is_available_but_missing_invalid_or_stale_samples_are_not() {
        let now = Instant::now();
        let mut panel = DrivePanel::default();
        let silent = WaveShaperMeter { count: 128, sample_rate: 48_000.0, latency_samples: 64, ..Default::default() };
        assert!(panel.meter.is_none());
        panel.accept_meter(silent, now);
        assert_eq!(panel.meter, Some(silent));
        panel.reset_stale(now + Duration::from_millis(249));
        assert_eq!(panel.meter, Some(silent));
        panel.reset_stale(now + Duration::from_millis(250));
        assert!(panel.meter.is_none());
        for invalid in [WaveShaperMeter { count: 129, ..silent }, WaveShaperMeter { sample_rate: f32::NAN, ..silent }, WaveShaperMeter::default()] {
            panel.accept_meter(silent, now);
            panel.accept_meter(invalid, now);
            assert!(panel.meter.is_none());
        }
        let mut invalid = silent;
        invalid.output[127] = f32::INFINITY;
        panel.accept_meter(invalid, now);
        assert!(panel.meter.is_none());
    }

    #[test]
    fn metering_reset_preserves_details_page_and_fresh_sound_is_explicit_modern() {
        let mut panel = DrivePanel::default();
        panel.update(DriveMsg::Page(TrackId::new(), EffectId::new(), DrivePage::Quality));
        panel.reset_metering();
        assert_eq!(panel.page, DrivePage::Quality);
        let fresh = fresh_sound();
        assert_eq!(fresh.len(), param::COUNT);
        for (index, descriptor) in vibez_dsp::drive::DRIVE_PARAMS.iter().enumerate() {
            assert_eq!(fresh[index], if index == param::CHARACTER { 1.0 } else { descriptor.default });
        }
        assert_eq!(vibez_dsp::drive::DRIVE_PARAMS[param::CHARACTER].default, 0.0);
    }
}
