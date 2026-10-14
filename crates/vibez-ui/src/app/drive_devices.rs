//! Routes native Drive page choices and generation-checked measured aligned waveform snapshots.

use super::*;
use crate::domains::drive::{is_native_drive, DriveMsg, DrivePanel};
use vibez_core::{
    effect::WaveShaperMeter,
    id::{EffectId, TrackId},
};

impl App {
    pub(super) fn update_drive_control(&mut self, message: DriveMsg) -> Task<Message> {
        let (track, effect) = message.destination();
        if self.state.find_track(track).is_some_and(|track| {
            track
                .effects
                .iter()
                .any(|candidate| candidate.id == effect && is_native_drive(candidate))
        }) {
            self.drive_panels.entry(effect).or_default().update(message);
        }
        Task::none()
    }
    pub(super) fn drive_panel(&self, effect: EffectId) -> &DrivePanel {
        self.drive_panels
            .get(&effect)
            .unwrap_or(&self.empty_drive_panel)
    }
    pub(super) fn poll_drive_panels(&mut self, selected: Option<TrackId>, now: std::time::Instant) {
        let project = &self.state.project_tracks;
        self.drive_panels.retain(|id, _| {
            project
                .tracks
                .iter()
                .chain(&project.buses)
                .chain(std::iter::once(&project.master))
                .any(|track| {
                    track
                        .effects
                        .iter()
                        .any(|effect| effect.id == *id && is_native_drive(effect))
                })
        });
        if let Some(track) = selected.and_then(|track| self.state.find_track(track)) {
            for effect in track
                .effects
                .iter()
                .filter(|effect| is_native_drive(effect))
            {
                let panel = self.drive_panels.entry(effect.id).or_default();
                if effect.bypass {
                    panel.reset_metering();
                } else {
                    panel.reset_stale(now);
                }
            }
        }
    }
    pub(super) fn accept_drive_waveshaper(
        &mut self,
        track: TrackId,
        effect: EffectId,
        generation: u64,
        meter: WaveShaperMeter,
    ) {
        if self.device_meters.channel != Some(track) || self.device_meters.generation != generation
        {
            return;
        }
        let Some(device) = self.state.find_track(track).and_then(|track| {
            track
                .effects
                .iter()
                .find(|candidate| candidate.id == effect && is_native_drive(candidate))
        }) else {
            return;
        };
        let panel = self.drive_panels.entry(effect).or_default();
        if device.bypass {
            panel.reset_metering();
        } else {
            panel.accept_meter(meter, std::time::Instant::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::{
        device_controls::DeviceControlMsg, devices::DevicesMsg, drive::DrivePage,
    };
    use crate::state::{DetailPanelTab, ProjectTrack, Workspace};
    use vibez_core::effect::EffectType;

    fn drive_app() -> (App, TrackId, EffectId) {
        let mut app = super::super::test_support::app();
        app.state.view.workspace = Workspace::Arrange;
        app.state.view.detail_panel_tab = DetailPanelTab::Devices;
        let channel = ProjectTrack::new(TrackId::new(), "Drive".into(), 3);
        let track = channel.id;
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(channel);
        let _ = app.update(Message::Devices(DevicesMsg::AddEffect(
            track,
            EffectType::Drive,
        )));
        let effect = app.state.find_track(track).unwrap().effects[0].id;
        app.state.arrangement.selected_track = Some(track);
        app.poll_device_meters();
        app.state.project.history = Default::default();
        (app, track, effect)
    }

    #[test]
    fn measured_waveforms_require_current_selection_and_clear_on_bypass_and_removal() {
        let (mut app, track, effect) = drive_app();
        let generation = app.device_meters.generation;
        assert_eq!(app.device_meters.channel, Some(track));
        let meter = WaveShaperMeter {
            count: 128,
            sample_rate: 48_000.0,
            latency_samples: 64,
            first_output_sample: 246,
            ..Default::default()
        };
        app.accept_drive_waveshaper(track, effect, generation.wrapping_sub(1), meter);
        assert!(app.drive_panel(effect).meter.is_none());
        app.accept_drive_waveshaper(track, effect, generation, meter);
        assert_eq!(app.drive_panel(effect).meter, Some(meter));
        app.state.find_track_mut(track).unwrap().effects[0].bypass = true;
        app.accept_drive_waveshaper(track, effect, generation, meter);
        assert!(app.drive_panel(effect).meter.is_none());
        app.state.find_track_mut(track).unwrap().effects[0].bypass = false;
        app.accept_drive_waveshaper(track, effect, generation, meter);
        app.state.arrangement.selected_track = None;
        app.poll_device_meters();
        assert!(app.drive_panel(effect).meter.is_none());
        app.accept_drive_waveshaper(track, effect, generation, meter);
        assert!(app.drive_panel(effect).meter.is_none());
        app.state.arrangement.selected_track = Some(track);
        app.poll_device_meters();
        let generation = app.device_meters.generation;
        app.accept_drive_waveshaper(track, effect, generation, meter);
        let _ = app.update(Message::Devices(DevicesMsg::RemoveEffect(track, effect)));
        app.poll_device_meters();
        app.accept_drive_waveshaper(track, effect, generation, meter);
        assert!(!app.drive_panels.contains_key(&effect));
    }

    #[test]
    fn rounded_drive_readout_is_a_noop_and_decibel_edits_undo_without_closing_details() {
        let (mut app, track, effect) = drive_app();
        let initial = app.state.find_track(track).unwrap().effects[0].params[0];
        let _ = app.update(Message::DeviceControl(DeviceControlMsg::ToggleAdvanced(
            track, effect,
        )));
        let _ = app.update(Message::DriveControl(DriveMsg::Page(
            track,
            effect,
            DrivePage::Quality,
        )));
        let shown = crate::domains::drive_values::DriveValue::Amount.display(initial);
        let _ = app.update(Message::DeviceControl(DeviceControlMsg::Input(
            track, effect, 0, shown,
        )));
        let _ = app.update(Message::DeviceControl(DeviceControlMsg::Submit(
            track, effect, 0,
        )));
        assert!(!app.state.project.history.can_undo());
        assert_eq!(
            app.state.find_track(track).unwrap().effects[0].params[0],
            initial
        );
        let _ = app.update(Message::DeviceControl(DeviceControlMsg::Input(
            track,
            effect,
            0,
            "6 dB".into(),
        )));
        let _ = app.update(Message::DeviceControl(DeviceControlMsg::Submit(
            track, effect, 0,
        )));
        assert!(
            (app.state.find_track(track).unwrap().effects[0].params[0] - 0.185_254_93).abs() < 1e-6
        );
        let snapshot = app.state.project.history.pop_undo().unwrap();
        assert!(!app.state.project.history.can_undo());
        app.apply_snapshot(snapshot);
        assert_eq!(
            app.state.find_track(track).unwrap().effects[0].params[0],
            initial
        );
        assert!(app.device_controls[&effect].advanced);
        assert_eq!(app.drive_panel(effect).page, DrivePage::Quality);
        assert!(app.drive_panel(effect).meter.is_none());
    }
}
