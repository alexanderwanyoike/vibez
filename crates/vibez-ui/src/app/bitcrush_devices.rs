//! Routes generation-checked native Bitcrush packets within the selected prepared owner lifecycle.

use super::*;
use crate::domains::bitcrush::{is_native_bitcrush, BitcrushMsg};
use crate::domains::bitcrush_layout::DetailPage;
use vibez_core::{
    effect::SampleHoldMeter,
    id::{EffectId, TrackId},
};

impl App {
    pub(super) fn update_bitcrush_control(&mut self, message: BitcrushMsg) -> Task<Message> {
        let (track, effect) = message.destination();
        if self.state.find_track(track).is_some_and(|track| {
            track.effects.iter().any(|candidate| candidate.id == effect && is_native_bitcrush(candidate))
        }) {
            self.bitcrush_panels.entry(effect).or_default().update(message);
        }
        Task::none()
    }

    pub(super) fn bitcrush_page(&self, effect: EffectId) -> DetailPage {
        self.bitcrush_panels.get(&effect).map_or(DetailPage::Main, |panel| panel.page)
    }

    pub(super) fn bitcrush_sample_hold(&self, effect: EffectId) -> Option<SampleHoldMeter> {
        self.bitcrush_panels.get(&effect)?.meter
    }

    pub(super) fn poll_bitcrush_panels(
        &mut self,
        selected: Option<TrackId>,
        now: std::time::Instant,
    ) {
        let project = &self.state.project_tracks;
        self.bitcrush_panels.retain(|id, _| {
            project
                .tracks
                .iter()
                .chain(&project.buses)
                .chain(std::iter::once(&project.master))
                .any(|track| {
                    track
                        .effects
                        .iter()
                        .any(|effect| effect.id == *id && is_native_bitcrush(effect))
                })
        });
        for track in project
            .tracks
            .iter()
            .chain(&project.buses)
            .chain(std::iter::once(&project.master))
        {
            for effect in track.effects.iter().filter(|effect| is_native_bitcrush(effect)) {
                let Some(panel) = self.bitcrush_panels.get_mut(&effect.id) else {
                    continue;
                };
                let prepared_rate = self.cmd_tx.prepared_effect_sample_rate(track.id, effect.id);
                if selected != Some(track.id)
                    || effect.bypass
                    || effect.reconfiguration_failed
                    || prepared_rate.is_none()
                    || panel.meter.is_some_and(|meter| {
                        Some(meter.sample_rate) != prepared_rate.map(|rate| rate as f32)
                    })
                {
                    panel.reset_metering();
                } else {
                    panel.reset_stale(now);
                }
            }
        }
    }

    pub(super) fn accept_bitcrush_sample_hold(
        &mut self,
        track: TrackId,
        effect: EffectId,
        generation: u64,
        meter: SampleHoldMeter,
    ) {
        if self.device_meters.channel != Some(track) || self.device_meters.generation != generation
        {
            return;
        }
        let Some(device) = self.state.find_track(track).and_then(|track| {
            track
                .effects
                .iter()
                .find(|candidate| candidate.id == effect && is_native_bitcrush(candidate))
        }) else {
            return;
        };
        let prepared_rate = self.cmd_tx.prepared_effect_sample_rate(track, effect);
        let panel = self.bitcrush_panels.entry(effect).or_default();
        if device.bypass
            || device.reconfiguration_failed
            || prepared_rate.is_none()
            || Some(meter.sample_rate) != prepared_rate.map(|rate| rate as f32)
        {
            panel.reset_metering();
        } else {
            panel.accept_meter(meter, std::time::Instant::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::devices::DevicesMsg;
    use crate::state::{DetailPanelTab, ProjectTrack, Workspace};
    use vibez_core::effect::EffectType;

    #[test]
    fn sample_hold_event_receiver_keeps_packet_chronology_and_rejects_retired_generations() {
        let mut app = super::super::test_support::app();
        app.state.transport.sample_rate = 48_000;
        app.send_command(EngineCommand::SetSampleRate(48_000));
        app.state.view.workspace = Workspace::Arrange;
        app.state.view.detail_panel_tab = DetailPanelTab::Devices;
        let channel = ProjectTrack::new(TrackId::new(), "Bitcrush".into(), 3);
        let track = channel.id;
        Arc::make_mut(&mut app.state.project_tracks).tracks.push(channel);
        let _ = app.update(Message::Devices(DevicesMsg::AddEffect(track, EffectType::Bitcrush)));
        let effect = app.state.find_track(track).unwrap().effects[0].id;
        app.state.arrangement.selected_track = Some(track);
        app.poll_device_meters();
        let generation = app.device_meters.generation;
        let mut meter = SampleHoldMeter {
            count: 2,
            sample_rate: 48_000.0,
            first_source_frame: 8192,
            primary_interval_frames: 4,
            primary_acquisitions: [1, 0],
            ..Default::default()
        };
        meter.input[..2].copy_from_slice(&[0.125, -0.25]);
        meter.output[..2].copy_from_slice(&[0.0, 0.0625]);
        let (mut events, receiver) = rtrb::RingBuffer::new(4);
        app.event_rx = Some(receiver);
        events.push(EngineEvent::EffectSampleHold {
            track_id: track,
            effect_id: effect,
            generation,
            meter,
        }).unwrap();
        app.poll_engine_events();
        assert_eq!(app.bitcrush_sample_hold(effect), Some(meter));
        app.restart_device_metering();
        events.push(EngineEvent::EffectSampleHold {
            track_id: track,
            effect_id: effect,
            generation,
            meter,
        }).unwrap();
        app.poll_engine_events();
        assert!(app.bitcrush_sample_hold(effect).is_none());
    }
}
