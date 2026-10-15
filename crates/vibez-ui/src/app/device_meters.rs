//! Coordinates the visible insert meter subscription and routes its readings.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use vibez_core::id::{EffectId, TrackId};
use vibez_engine::metering::MeterValues;

use crate::domains::compressor::is_native_compressor;
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
    pub input: MeterValues,
    pub output: MeterValues,
    last_meter_at: Option<Instant>,
}

impl MeterReadings {
    fn accept(&mut self, input: MeterValues, output: MeterValues, now: Instant) {
        self.input = input;
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
    is_native_gain(effect) || is_native_compressor(effect)
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
        self.poll_compressor_panels(wanted, Instant::now());
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
                } else if is_native_compressor(effect) {
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
        for panel in self.compressor_panels.values_mut() {
            panel.reset_metering();
        }
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
        self.gain_devices.panels.clear();
        self.compressor_panels.clear();
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
        } else if is_native_compressor(effect) {
            self.device_meters
                .readings
                .entry(effect_id)
                .or_default()
                .accept(input, output, now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::device_controls::DeviceControlMsg;
    use crate::domains::devices::DevicesMsg;
    use crate::domains::gain::GainMsg;
    use crate::state::ProjectTrack;
    use vibez_core::effect::EffectType;

    fn metered_app() -> (App, TrackId, EffectId, EffectId) {
        let mut app = super::super::test_support::app();
        let track = ProjectTrack::new(TrackId::new(), "Inserts".into(), 3);
        let track_id = track.id;
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(track);
        for effect_type in [EffectType::Gain, EffectType::Compressor] {
            let _ = app.update(Message::Devices(DevicesMsg::AddEffect(
                track_id,
                effect_type,
            )));
        }
        let track = app.state.find_track(track_id).unwrap();
        let gain = track.effects[0].id;
        let compressor = track.effects[1].id;
        app.state.arrangement.selected_track = Some(track_id);
        app.state.view.detail_panel_tab = DetailPanelTab::Devices;
        app.poll_device_meters();
        (app, track_id, gain, compressor)
    }

    fn levels() -> MeterValues {
        MeterValues {
            peak_l: 1.1,
            peak_r: 0.5,
            rms_l: 0.4,
            rms_r: 0.3,
        }
    }

    #[test]
    fn shared_subscription_preserves_gain_history_and_device_disclosures() {
        let (mut app, track, gain, compressor) = metered_app();
        let generation = app.device_meters.generation;
        for effect in [gain, compressor] {
            app.accept_device_meter(track, effect, generation, levels(), levels());
        }
        let _ = app.update(Message::GainControl(GainMsg::ToggleAdvanced(gain)));
        let _ = app.update(Message::DeviceControl(DeviceControlMsg::ToggleAdvanced(
            track, compressor,
        )));
        let frequency = vibez_dsp::compressor::param::HP_FREQUENCY;
        let _ = app.update(Message::DeviceControl(DeviceControlMsg::Input(
            track,
            compressor,
            frequency,
            "invalid".into(),
        )));
        let _ = app.update(Message::DeviceControl(DeviceControlMsg::Submit(
            track, compressor, frequency,
        )));
        assert!(app.device_controls[&compressor].errors.contains(&frequency));
        let panel = &app.gain_devices.panels[&gain];
        assert_eq!(panel.history.len(), 1);
        assert!(panel.clip_latched);
        assert_eq!(panel.output, levels());
        assert_eq!(app.device_meters.readings[&compressor].output, levels());
        assert!(!app.gain_devices.panels.contains_key(&compressor));

        app.restart_device_metering();

        let panel = &app.gain_devices.panels[&gain];
        assert!(panel.advanced);
        assert!(panel.history.is_empty());
        assert!(!panel.clip_latched);
        assert_eq!(panel.output, MeterValues::SILENT);
        assert!(app.device_controls[&compressor].advanced);
        assert!(app.device_controls[&compressor].entries.is_empty());
        assert!(app.device_controls[&compressor].errors.is_empty());
        assert_eq!(
            app.device_meters.readings[&compressor].output,
            MeterValues::SILENT
        );
    }

    #[test]
    fn late_compressor_readings_cannot_cross_selection_removal_or_project_reset() {
        let (mut app, track, gain, compressor) = metered_app();
        let stale_generation = app.device_meters.generation;
        app.accept_device_meter(track, compressor, stale_generation, levels(), levels());
        app.state.arrangement.selected_track = None;
        app.poll_device_meters();
        assert_eq!(
            app.device_meters.readings[&compressor].output,
            MeterValues::SILENT
        );

        app.state.arrangement.selected_track = Some(track);
        app.poll_device_meters();
        app.accept_device_meter(track, compressor, stale_generation, levels(), levels());
        assert_eq!(
            app.device_meters.readings[&compressor].output,
            MeterValues::SILENT
        );
        let generation = app.device_meters.generation;
        app.accept_device_meter(track, compressor, generation, levels(), levels());
        assert_eq!(app.device_meters.readings[&compressor].output, levels());
        let _ = app.update(Message::DeviceControl(DeviceControlMsg::ToggleAdvanced(
            track, compressor,
        )));

        let _ = app.update(Message::Devices(DevicesMsg::RemoveEffect(track, compressor)));
        app.poll_device_meters();
        app.accept_device_meter(track, compressor, generation, levels(), levels());
        assert!(!app.device_meters.readings.contains_key(&compressor));
        assert!(!app.device_controls.contains_key(&compressor));

        let generation = app.device_meters.generation;
        app.reset_device_meters();
        app.accept_device_meter(track, gain, generation, levels(), levels());
        assert!(app.device_meters.readings.is_empty());
        assert!(app.gain_devices.panels.is_empty());
        assert!(app.device_controls.is_empty());
    }

    #[test]
    fn compressor_readings_expire_at_the_meter_timeout() {
        let now = Instant::now();
        let mut readings = MeterReadings::default();
        readings.accept(levels(), levels(), now);
        readings.reset_stale(now + Duration::from_millis(249));
        assert_eq!(readings.output, levels());
        readings.reset_stale(now + Duration::from_millis(250));
        assert_eq!(readings.output, MeterValues::SILENT);
    }
}
