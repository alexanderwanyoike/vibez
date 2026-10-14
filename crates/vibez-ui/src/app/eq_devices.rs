//! Maintains selected native EQ panels and drains their genuine insert analyser tap.

use super::*;
use crate::domains::eq::{first_native_eq, is_native_eq, EqMsg, EqPanel};
use vibez_core::id::EffectId;

impl App {
    pub(super) fn update_eq_control(&mut self, message: EqMsg) -> Task<Message> {
        let (track, effect) = message.destination();
        if !self.state.find_track(track).is_some_and(|track| {
            track
                .effects
                .iter()
                .any(|candidate| candidate.id == effect && is_native_eq(candidate))
        }) {
            return Task::none();
        }
        self.eq_spectrum.focus = Some((track, effect));
        self.eq_panels.entry(effect).or_default().update(message);
        Task::none()
    }

    pub(super) fn eq_panel(&self, effect: EffectId) -> &EqPanel {
        self.eq_panels.get(&effect).unwrap_or(&self.empty_eq_panel)
    }

    pub(super) fn poll_eq_spectrum(&mut self) {
        let project = &self.state.project_tracks;
        self.eq_panels.retain(|id, _| {
            project
                .tracks
                .iter()
                .chain(&project.buses)
                .chain(std::iter::once(&project.master))
                .any(|track| {
                    track
                        .effects
                        .iter()
                        .any(|effect| effect.id == *id && is_native_eq(effect))
                })
        });
        let wanted = self
            .state
            .active_timeline_editor()
            .selected_track
            .and_then(|track_id| {
                let track = self.state.find_track(track_id)?;
                let focused = (self.state.view.workspace != crate::state::Workspace::Mix
                    && self.state.view.detail_panel_tab == crate::state::DetailPanelTab::Devices)
                    .then_some(self.eq_spectrum.focus)
                    .flatten()
                    .filter(|(owner, _)| *owner == track_id)
                    .and_then(|(_, id)| {
                        track
                            .effects
                            .iter()
                            .find(|effect| effect.id == id && is_native_eq(effect))
                    });
                let effect = focused.or_else(|| first_native_eq(track))?;
                Some((track_id, effect.id, self.eq_panel(effect.id).position()))
            });
        if self.eq_spectrum.retarget(wanted) {
            if let Some(target) = self.eq_spectrum.target {
                self.eq_panels
                    .entry(target.effect_id)
                    .or_default()
                    .spectrum
                    .reset();
            }
            self.send_command(EngineCommand::SetEffectSpectrumTap(self.eq_spectrum.target));
        }
        let Some(rx) = self.effect_spectrum_rx.as_mut() else {
            return;
        };
        let now = std::time::Instant::now();
        let mut chunk = [0.0; 512];
        let mut count = 0;
        while let Ok(sample) = rx.pop() {
            if let (Some(value), Some(target)) =
                (self.eq_spectrum.accept(sample), self.eq_spectrum.target)
            {
                let panel = self.eq_panels.entry(target.effect_id).or_default();
                panel.last_sample_at = Some(now);
                if !panel.frozen {
                    chunk[count] = value;
                    count += 1;
                    if count == chunk.len() {
                        panel.spectrum.ingest(&chunk);
                        count = 0;
                    }
                }
            }
        }
        if let Some(target) = self.eq_spectrum.target {
            let panel = self.eq_panels.entry(target.effect_id).or_default();
            if !panel.frozen {
                panel.spectrum.ingest(&chunk[..count]);
                if panel
                    .last_sample_at
                    .is_some_and(|last| last.elapsed() >= std::time::Duration::from_millis(250))
                {
                    panel.spectrum.reset();
                }
                panel
                    .spectrum
                    .analyse(self.state.transport.sample_rate as f32);
            }
        }
    }

    pub(super) fn restart_eq_spectrum(&mut self) {
        self.eq_spectrum.retarget(None);
        for panel in self.eq_panels.values_mut() {
            panel.spectrum.reset();
            panel.last_sample_at = None;
        }
        self.send_command(EngineCommand::SetEffectSpectrumTap(None));
    }

    pub(super) fn reset_eq_panels(&mut self) {
        self.eq_panels.clear();
        self.eq_spectrum.focus = None;
        self.eq_spectrum.retarget(None);
        self.send_command(EngineCommand::SetEffectSpectrumTap(None));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::devices::DevicesMsg;
    use crate::state::{ProjectTrack, Workspace};
    use vibez_core::{effect::EffectType, id::TrackId};
    use vibez_engine::effect_spectrum::{EffectSpectrumPosition, EffectSpectrumSample};

    fn app_with_eq() -> (App, TrackId, EffectId) {
        let mut app = super::super::test_support::app();
        app.state.view.workspace = Workspace::Arrange;
        let track = ProjectTrack::new(TrackId::new(), "EQ".into(), 3);
        let id = track.id;
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(track);
        let _ = app.update(Message::Devices(DevicesMsg::AddEffect(id, EffectType::Eq)));
        let effect = first_native_eq(app.state.find_track(id).unwrap())
            .unwrap()
            .id;
        app.state.arrangement.selected_track = Some(id);
        (app, id, effect)
    }

    #[test]
    fn frozen_analyser_drains_ring_then_source_change_discards_old_generation() {
        let (mut app, track, effect) = app_with_eq();
        let (mut producer, consumer) = rtrb::RingBuffer::new(4096);
        app.effect_spectrum_rx = Some(consumer);
        app.poll_eq_spectrum();
        let generation = app.eq_spectrum.generation;
        for i in 0..2048 {
            producer
                .push(EffectSpectrumSample {
                    generation,
                    value: (i as f32 * 0.1).sin() * 0.25,
                })
                .unwrap();
        }
        app.poll_eq_spectrum();
        assert!(app.eq_panel(effect).spectrum.active);
        let bins = app.eq_panel(effect).spectrum.bins.clone();
        let _ = app.update_eq_control(EqMsg::Freeze(track, effect));
        for _ in 0..4096 {
            producer
                .push(EffectSpectrumSample {
                    generation,
                    value: 1.0,
                })
                .unwrap();
        }
        app.poll_eq_spectrum();
        assert_eq!(producer.slots(), 4096);
        assert_eq!(app.eq_panel(effect).spectrum.bins, bins);
        let _ = app.update_eq_control(EqMsg::Source(track, effect, EffectSpectrumPosition::Output));
        app.poll_eq_spectrum();
        assert_ne!(app.eq_spectrum.generation, generation);
        assert!(!app.eq_panel(effect).spectrum.active);
        let _ = app.update_eq_control(EqMsg::Freeze(track, effect));
        for _ in 0..2048 {
            producer
                .push(EffectSpectrumSample {
                    generation,
                    value: 1.0,
                })
                .unwrap();
        }
        app.poll_eq_spectrum();
        assert!(!app.eq_panel(effect).spectrum.active);
        assert_eq!(producer.slots(), 4096);
    }

    #[test]
    fn selection_and_undo_preserve_panel_choices_but_removal_and_project_reset_prune() {
        let (mut app, track, effect) = app_with_eq();
        let _ = app.update_eq_control(EqMsg::SelectBand(track, effect, 2));
        let _ = app.update_eq_control(EqMsg::Source(track, effect, EffectSpectrumPosition::Output));
        let _ = app.update_eq_control(EqMsg::Freeze(track, effect));
        app.poll_eq_spectrum();
        let generation = app.eq_spectrum.generation;
        let _ = app.update(Message::set_effect_param(track, effect, 6, 3.0));
        let snapshot = app.state.project.history.pop_undo().unwrap();
        app.apply_snapshot(snapshot);
        app.poll_eq_spectrum();
        assert!(app.eq_spectrum.generation > generation);
        assert_eq!(app.eq_panel(effect).selected_band, 2);
        assert!(app.eq_panel(effect).post && app.eq_panel(effect).frozen);
        app.state.arrangement.selected_track = None;
        app.poll_eq_spectrum();
        assert!(app.eq_spectrum.target.is_none());
        assert_eq!(app.eq_panel(effect).selected_band, 2);
        app.state.find_track_mut(track).unwrap().effects.clear();
        app.poll_eq_spectrum();
        assert!(!app.eq_panels.contains_key(&effect));
        app.eq_panels.entry(effect).or_default();
        app.reset_device_controls();
        assert!(app.eq_panels.is_empty());
    }

    #[test]
    fn mix_and_devices_follow_same_first_native_eq_for_tracks_buses_and_master() {
        let (mut app, track, effect) = app_with_eq();
        let bus = ProjectTrack::new(TrackId::new(), "Return".into(), 4);
        let bus_id = bus.id;
        Arc::make_mut(&mut app.state.project_tracks).buses.push(bus);
        let master = app.state.project_tracks.master.id;
        for id in [bus_id, master] {
            let _ = app.update(Message::Devices(DevicesMsg::AddEffect(id, EffectType::Eq)));
        }
        for (workspace, detail) in [
            (Workspace::Mix, crate::state::DetailPanelTab::Clip),
            (Workspace::Arrange, crate::state::DetailPanelTab::Devices),
        ] {
            app.state.view.workspace = workspace;
            app.state.view.detail_panel_tab = detail;
            for id in [track, bus_id, master] {
                app.state.arrangement.selected_track = Some(id);
                app.poll_eq_spectrum();
                let console = first_native_eq(app.state.find_track(id).unwrap())
                    .unwrap()
                    .id;
                assert_eq!(app.eq_spectrum.target.unwrap().effect_id, console);
                assert_eq!(app.eq_spectrum.target.unwrap().track_id, id);
            }
        }
        assert_eq!(
            first_native_eq(app.state.find_track(track).unwrap())
                .unwrap()
                .id,
            effect
        );
    }

    #[test]
    fn devices_can_analyse_second_eq_while_mix_keeps_console_identity() {
        let (mut app, track, first) = app_with_eq();
        app.state.view.detail_panel_tab = crate::state::DetailPanelTab::Devices;
        let _ = app.update(Message::Devices(DevicesMsg::AddEffect(
            track,
            EffectType::Eq,
        )));
        let second = app.state.find_track(track).unwrap().effects[1].id;
        let _ = app.update_eq_control(EqMsg::Source(track, second, EffectSpectrumPosition::Output));
        app.poll_eq_spectrum();
        assert_eq!(app.eq_spectrum.target.unwrap().effect_id, second);
        assert_eq!(
            app.eq_spectrum.target.unwrap().position,
            EffectSpectrumPosition::Output
        );
        app.state.view.workspace = Workspace::Mix;
        app.poll_eq_spectrum();
        assert_eq!(app.eq_spectrum.target.unwrap().effect_id, first);
        app.state.view.workspace = Workspace::Arrange;
        app.poll_eq_spectrum();
        assert_eq!(app.eq_spectrum.target.unwrap().effect_id, second);
        app.state
            .find_track_mut(track)
            .unwrap()
            .effects
            .retain(|effect| effect.id != second);
        app.poll_eq_spectrum();
        assert_eq!(app.eq_spectrum.target.unwrap().effect_id, first);
        assert_eq!(
            first_native_eq(app.state.find_track(track).unwrap())
                .unwrap()
                .id,
            first
        );
    }
}
