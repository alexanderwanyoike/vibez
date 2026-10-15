//! Native EQ cards on the visible device chain: their panel state and the
//! analyser that listens to one EQ's own input or output.

use std::collections::HashMap;

use rtrb::Consumer;
use vibez_core::id::{EffectId, TrackId};
use vibez_engine::effect_spectrum::{
    EffectSpectrumPosition, EffectSpectrumSample, EffectSpectrumTarget,
};

use crate::domains::eq::{is_native_eq, EqMsg, EqPanelState, SpectrumSide};
use crate::spectrum::SpectrumState;
use crate::state::DetailPanelTab;

use super::*;

#[derive(Default)]
pub(super) struct EqDevices {
    pub panels: HashMap<EffectId, EqPanelState>,
    pub empty_panel: EqPanelState,
    /// The EQ the analyser listens to: the last one touched, else the first.
    pub focus: Option<EffectId>,
    pub spectrum_rx: Option<Consumer<EffectSpectrumSample>>,
    pub target: Option<EffectSpectrumTarget>,
    pub generation: u64,
    pub analyser: SpectrumState,
}

impl EqDevices {
    pub fn panel(&self, effect_id: EffectId) -> &EqPanelState {
        self.panels.get(&effect_id).unwrap_or(&self.empty_panel)
    }

    /// Analyser bins for this EQ's card, when it is the one being listened to.
    pub fn spectrum_for(&self, effect_id: EffectId) -> Option<&SpectrumState> {
        (self.target.map(|target| target.effect_id) == Some(effect_id)).then_some(&self.analyser)
    }
}

impl App {
    pub(super) fn update_eq(&mut self, message: EqMsg) -> Task<Message> {
        let effect_id = message.effect_id();
        self.eq_devices
            .panels
            .entry(effect_id)
            .or_default()
            .update(message);
        self.eq_devices.focus = Some(effect_id);
        Task::none()
    }

    fn wanted_eq_tap(&self) -> Option<(TrackId, EffectId, SpectrumSide)> {
        if self.state.view.detail_panel_tab != DetailPanelTab::Devices {
            return None;
        }
        let track_id = self.state.active_timeline_editor().selected_track?;
        let track = self.state.find_track(track_id)?;
        let mut eqs = track.effects.iter().filter(|effect| is_native_eq(effect));
        let effect = eqs
            .clone()
            .find(|effect| Some(effect.id) == self.eq_devices.focus)
            .or_else(|| eqs.next())?;
        Some((track_id, effect.id, self.eq_devices.panel(effect.id).side))
    }

    pub(super) fn poll_eq_devices(&mut self) {
        let wanted = self.wanted_eq_tap();
        let current = self
            .eq_devices
            .target
            .map(|target| (target.track_id, target.effect_id, target.position));
        let wanted_position = wanted.map(|(track_id, effect_id, side)| {
            let position = match side {
                SpectrumSide::Pre => EffectSpectrumPosition::Input,
                SpectrumSide::Post => EffectSpectrumPosition::Output,
            };
            (track_id, effect_id, position)
        });
        if wanted_position != current {
            self.eq_devices.generation = self.eq_devices.generation.wrapping_add(1);
            let generation = self.eq_devices.generation;
            self.eq_devices.target =
                wanted_position.map(|(track_id, effect_id, position)| EffectSpectrumTarget {
                    track_id,
                    effect_id,
                    position,
                    generation,
                });
            self.eq_devices.analyser.reset();
            self.send_command(EngineCommand::SetEffectSpectrumTap(self.eq_devices.target));
        }
        let frozen = self
            .eq_devices
            .target
            .is_some_and(|target| self.eq_devices.panel(target.effect_id).frozen);
        let generation = self.eq_devices.generation;
        let devices = &mut self.eq_devices;
        if let Some(rx) = devices.spectrum_rx.as_mut() {
            let mut chunk = [0.0f32; 512];
            loop {
                let mut filled = 0;
                while filled < chunk.len() {
                    let Ok(sample) = rx.pop() else {
                        break;
                    };
                    if sample.generation == generation {
                        chunk[filled] = sample.value;
                        filled += 1;
                    }
                }
                if !frozen {
                    devices.analyser.ingest(&chunk[..filled]);
                }
                if filled < chunk.len() {
                    break;
                }
            }
        }
        if !frozen {
            devices
                .analyser
                .analyse(self.state.transport.sample_rate as f32);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::devices::DevicesMsg;
    use crate::state::ProjectTrack;
    use vibez_core::effect::EffectType;

    fn eq_app() -> (App, TrackId, EffectId, EffectId) {
        let mut app = super::super::test_support::app();
        let track = ProjectTrack::new(TrackId::new(), "Bus".into(), 2);
        let id = track.id;
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(track);
        for _ in 0..2 {
            let _ = app.update(Message::Devices(DevicesMsg::AddEffect(id, EffectType::Eq)));
        }
        let effects = &app.state.find_track(id).unwrap().effects;
        let (first, second) = (effects[0].id, effects[1].id);
        app.state.arrangement.selected_track = Some(id);
        app.state.view.detail_panel_tab = DetailPanelTab::Devices;
        (app, id, first, second)
    }

    #[test]
    fn the_analyser_listens_after_the_first_eq_until_another_is_touched() {
        let (mut app, track, first, second) = eq_app();
        app.poll_eq_devices();
        let target = app.eq_devices.target.unwrap();
        assert_eq!((target.track_id, target.effect_id), (track, first));
        assert_eq!(target.position, EffectSpectrumPosition::Output);

        let _ = app.update(Message::Eq(EqMsg::SetSpectrumSide(
            second,
            SpectrumSide::Pre,
        )));
        app.poll_eq_devices();
        let moved = app.eq_devices.target.unwrap();
        assert_eq!(moved.effect_id, second);
        assert_eq!(moved.position, EffectSpectrumPosition::Input);
        assert!(moved.generation != target.generation);
        assert!(app.eq_devices.spectrum_for(first).is_none());
        assert!(app.eq_devices.spectrum_for(second).is_some());
    }

    #[test]
    fn the_analyser_stops_listening_when_devices_are_hidden() {
        let (mut app, ..) = eq_app();
        app.poll_eq_devices();
        app.state.view.detail_panel_tab = DetailPanelTab::Clip;
        app.poll_eq_devices();
        assert!(app.eq_devices.target.is_none());
    }
}
