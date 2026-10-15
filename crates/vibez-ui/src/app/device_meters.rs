//! Coordinates the visible insert meter subscription and routes its readings.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use vibez_core::id::{EffectId, TrackId};
use vibez_engine::metering::MeterValues;

use crate::domains::gain::is_native_gain;
use crate::state::{DetailPanelTab, UiEffect};

use super::*;

#[derive(Default)]
pub(super) struct DeviceMeters {
    pub channel: Option<TrackId>,
    pub generation: u64,
    pub readings: HashMap<EffectId, MeterReadings>,
}

#[derive(Default)]
pub(super) struct MeterReadings {
    pub output: MeterValues,
    last_meter_at: Option<Instant>,
}

impl MeterReadings {
    pub(super) fn available_output(&self) -> Option<MeterValues> {
        self.last_meter_at
            .filter(|at| at.elapsed() < Duration::from_millis(250))
            .map(|_| self.output)
    }

    fn accept(&mut self, output: MeterValues, now: Instant) {
        if ![output.peak_l, output.peak_r, output.rms_l, output.rms_r]
            .into_iter().all(|value| value.is_finite() && value >= 0.0) {
            *self = Self::default();
            return;
        }
        self.output = output;
        self.last_meter_at = Some(now);
    }

    fn reset_stale(&mut self, now: Instant) {
        if self
            .last_meter_at
            .is_some_and(|last| now.saturating_duration_since(last) >= Duration::from_millis(250))
        {
            *self = Self::default();
        }
    }
}

fn is_metered_effect(effect: &UiEffect) -> bool {
    is_native_gain(effect) || crate::domains::delay_observation::is_native_delay(effect)
}

impl App {
    pub(super) fn poll_device_meters(&mut self) {
        let wanted = (self.state.view.detail_panel_tab == DetailPanelTab::Devices)
            .then(|| self.state.active_timeline_editor().selected_track)
            .flatten()
            .filter(|track_id| {
                self.state
                    .find_track(*track_id)
                    .is_some_and(|track| track.effects.iter().any(is_metered_effect))
            });
        if wanted != self.device_meters.channel {
            self.device_meters.channel = wanted;
            self.restart_device_metering();
        }
        self.prune_device_controls();
        self.poll_delay_devices();
        let project = &self.state.project_tracks;
        let effect_exists = |id: &EffectId| {
            project
                .tracks
                .iter()
                .chain(&project.buses)
                .chain(std::iter::once(&project.master))
                .any(|track| track.effects.iter().any(|effect| effect.id == *id))
        };
        self.gain_devices.panels.retain(|id, _| effect_exists(id));
        self.device_meters
            .readings
            .retain(|id, _| effect_exists(id));
        if let Some(track) = wanted.and_then(|track_id| self.state.find_track(track_id)) {
            let now = Instant::now();
            for effect in &track.effects {
                if is_native_gain(effect) {
                    self.gain_devices
                        .panels
                        .entry(effect.id)
                        .or_default()
                        .reset_stale(now);
                } else if is_metered_effect(effect) {
                    self.device_meters
                        .readings
                        .entry(effect.id)
                        .or_default()
                        .reset_stale(now);
                }
            }
        }
    }

    /// Replayed channels and changed selections invalidate queued readings,
    /// while disclosure state belongs to the device rather than the subscription.
    pub(super) fn restart_device_metering(&mut self) {
        self.discard_all_device_entries();
        self.restart_delay_responses();
        self.device_meters.generation = self.device_meters.generation.wrapping_add(1);
        for readings in self.device_meters.readings.values_mut() {
            *readings = MeterReadings::default();
        }
        for panel in self.gain_devices.panels.values_mut() {
            panel.restart_metering();
        }
        self.send_command(EngineCommand::SetEffectMeterChannel {
            track_id: self.device_meters.channel,
            generation: self.device_meters.generation,
        });
    }

    pub(super) fn reset_device_meters(&mut self) {
        self.device_meters.channel = None;
        self.device_meters.generation = self.device_meters.generation.wrapping_add(1);
        self.device_meters.readings.clear();
        self.restart_delay_responses();
        self.delay_devices.panels.clear();
        self.gain_devices.panels.clear();
        self.reset_device_controls();
        self.send_command(EngineCommand::SetEffectMeterChannel {
            track_id: None,
            generation: self.device_meters.generation,
        });
    }

    pub(super) fn accept_device_meter(
        &mut self,
        track_id: TrackId,
        effect_id: EffectId,
        generation: u64,
        input: MeterValues,
        output: MeterValues,
    ) {
        if self.device_meters.channel != Some(track_id)
            || self.device_meters.generation != generation
        {
            return;
        }
        let Some(effect) = self
            .state
            .find_track(track_id)
            .and_then(|track| track.effects.iter().find(|effect| effect.id == effect_id))
        else {
            return;
        };
        let now = Instant::now();
        if is_native_gain(effect) {
            self.gain_devices
                .panels
                .entry(effect_id)
                .or_default()
                .accept_meter(input, output, now);
        } else if is_metered_effect(effect) {
            self.device_meters
                .readings
                .entry(effect_id)
                .or_default()
                .accept(output, now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_output_clears_availability_instead_of_refreshing_the_meter() {
        let now = Instant::now();
        let valid = MeterValues { peak_l: 2.0, peak_r: 1.5, rms_l: 1.2, rms_r: 1.0 };
        for field in 0..4 {
            for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.01] {
                let mut meter = MeterReadings::default();
                meter.accept(valid, now);
                assert!(meter.available_output().is_some());
                let mut values = [valid.peak_l, valid.peak_r, valid.rms_l, valid.rms_r];
                values[field] = invalid;
                meter.accept(MeterValues { peak_l: values[0], peak_r: values[1],
                    rms_l: values[2], rms_r: values[3] }, now);
                assert!(meter.available_output().is_none(), "field {field}: {invalid}");
                meter.accept(MeterValues::SILENT, now);
                assert_eq!(meter.available_output(), Some(MeterValues::SILENT));
            }
        }
    }

    #[test]
    fn readings_expire_at_the_meter_timeout() {
        let now = Instant::now();
        let levels = MeterValues {
            peak_l: 1.1,
            peak_r: 0.5,
            rms_l: 0.4,
            rms_r: 0.3,
        };
        let mut readings = MeterReadings::default();
        readings.accept(levels, now);
        readings.reset_stale(now + Duration::from_millis(249));
        assert_eq!(readings.output, levels);
        readings.reset_stale(now + Duration::from_millis(250));
        assert_eq!(readings.output, MeterValues::SILENT);
    }
}
