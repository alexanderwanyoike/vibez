//! Returned device owners take the real main-thread resume or rejection path.

use super::*;
use crate::state::{ProjectTrack, UiEffect};
use vibez_core::{
    effect::EffectType,
    id::{EffectId, TrackId},
};
use vibez_engine::engine::reconfiguration::DeviceReconfiguration;

struct FailedRestart(std::thread::ThreadId);
impl vibez_dsp::effect::AudioEffect for FailedRestart {
    fn reconfigure_on_main_thread(&mut self) -> Result<(), String> {
        assert_eq!(self.0, std::thread::current().id());
        Err("Rejected restart".into())
    }
    fn effect_type(&self) -> EffectType {
        EffectType::Gain
    }
    fn param_descriptors(&self) -> &'static [vibez_core::effect::ParamDescriptor] {
        &[]
    }
    fn set_param(&mut self, _: usize, _: f32) -> bool {
        false
    }
    fn get_param(&self, _: usize) -> f32 {
        0.0
    }
    fn process(&mut self, _: &mut [f32], _: usize) {}
    fn reset(&mut self) {}
}

#[test]
fn returned_device_event_reconfigures_on_main_and_publishes_resume_or_reject_with_owner() {
    for fail in [false, true] {
        let mut app = super::test_support::app();
        let mut track = ProjectTrack::new(TrackId::new(), "Track".into(), 0);
        let id = EffectId::new();
        track.effects.push(UiEffect {
            reconfiguration_failed: false,
            id,
            effect_type: EffectType::Gain,
            bypass: false,
            params: vec![],
            descriptors: &[],
            plugin_name: Some("Returned device".into()),
            has_plugin_gui: false,
            plugin_ref: None,
            sidechains: vec![],
            inactive_sidechains: vec![],
            external_inputs: vec![],
            latency_samples: Some(521),
        });
        let track_id = track.id;
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(track);
        let effect: Box<dyn vibez_dsp::effect::AudioEffect> = if fail {
            Box::new(FailedRestart(std::thread::current().id()))
        } else {
            vibez_dsp::factory::create_effect(EffectType::Gain, 44100.0)
        };
        let (commands, mut consumer) = rtrb::RingBuffer::new(8);
        app.cmd_tx = crate::domains::EngineCommandQueue::new(commands);
        app.send_command(EngineCommand::AddPluginEffect {
            track_id,
            effect_id: id,
            effect,
            position: None,
        });
        let EngineCommand::AddPluginEffect { effect, .. } = consumer.pop().unwrap() else {
            panic!("device creation")
        };
        let (mut events, receiver) = rtrb::RingBuffer::new(8);
        app.event_rx = Some(receiver);
        events
            .push(EngineEvent::DeviceReconfiguration(
                DeviceReconfiguration::Effect {
                    handoff_id: 1,
                    reserved_effects: Vec::new(),
                    track_id,
                    position: 0,
                    slot: vibez_engine::mixer::EffectSlot {
                        id,
                        effect,
                        bypass: false,
                    },
                },
            ))
            .unwrap();
        app.poll_engine_events();
        let command = consumer.pop().unwrap();
        match command {
            EngineCommand::RejectDeviceReconfiguration { device, reason } if fail => {
                assert_eq!(device.effect_id(), Some(id));
                assert!(reason.contains("Returned device: Rejected restart"));
                assert_eq!(
                    app.state.project_tracks.tracks[0].effects[0].latency_samples,
                    None
                );
                assert!(app.state.project_tracks.tracks[0].effects[0].reconfiguration_failed);
                assert!(app
                    .sidechain_model()
                    .iter()
                    .any(|channel| channel.id == track_id
                        && channel.effects.iter().any(|effect| effect.id == id)));
            }
            EngineCommand::ResumeDeviceReconfiguration { device, routing } if !fail => {
                assert_eq!(device.effect_id(), Some(id));
                assert_eq!(
                    app.state.project_tracks.tracks[0].effects[0].latency_samples,
                    Some(0)
                );
                assert_eq!(routing.compensation.output_latency, 0);
            }
            _ => panic!("Unexpected ownership result"),
        }
        assert!(consumer.pop().is_err());
    }
}

struct RestartProbe(std::sync::Arc<std::sync::atomic::AtomicUsize>);
impl vibez_dsp::effect::AudioEffect for RestartProbe {
    fn reconfigure_on_main_thread(&mut self) -> Result<(), String> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }
    fn effect_type(&self) -> EffectType {
        EffectType::Gain
    }
    fn param_descriptors(&self) -> &'static [vibez_core::effect::ParamDescriptor] {
        &[]
    }
    fn set_param(&mut self, _: usize, _: f32) -> bool {
        false
    }
    fn get_param(&self, _: usize) -> f32 {
        0.0
    }
    fn process(&mut self, _: &mut [f32], _: usize) {}
    fn reset(&mut self) {}
}

#[test]
fn queued_replacement_or_project_reset_revokes_old_event_before_main_activation() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    for replace in [false, true] {
        let mut app = super::test_support::app();
        let mut track = ProjectTrack::new(TrackId::new(), "Reopened same IDs".into(), 0);
        let track_id = track.id;
        let effect_id = EffectId::new();
        let mut ui_effect = UiEffect {
            reconfiguration_failed: false,
            id: effect_id,
            effect_type: EffectType::Gain,
            bypass: false,
            params: vec![],
            descriptors: &[],
            plugin_name: Some("Current".into()),
            has_plugin_gui: false,
            plugin_ref: None,
            sidechains: vec![],
            inactive_sidechains: vec![],
            external_inputs: vec![],
            latency_samples: None,
        };
        ui_effect.latency_samples = Some(777);
        track.effects.push(ui_effect);
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(track);
        let restarts = Arc::new(AtomicUsize::new(0));
        let (producer, mut consumer) = rtrb::RingBuffer::new(1);
        app.cmd_tx = crate::domains::EngineCommandQueue::new(producer);
        app.send_command(EngineCommand::AddPluginEffect {
            track_id,
            effect_id,
            effect: Box::new(RestartProbe(Arc::clone(&restarts))),
            position: None,
        });
        let EngineCommand::AddPluginEffect { effect, .. } = consumer.pop().unwrap() else {
            panic!("initial owner")
        };
        let device = DeviceReconfiguration::Effect {
            handoff_id: 1,
            reserved_effects: Vec::new(),
            track_id,
            position: 0,
            slot: vibez_engine::mixer::EffectSlot {
                id: effect_id,
                effect,
                bypass: false,
            },
        };
        app.send_command(EngineCommand::Play);
        if replace {
            app.send_command(EngineCommand::AddPluginEffect {
                track_id,
                effect_id,
                effect: Box::new(RestartProbe(Arc::new(AtomicUsize::new(0)))),
                position: None,
            });
        } else {
            let reopened = app.state.project_tracks.tracks[0].clone();
            app.clear_project_runtime();
            Arc::make_mut(&mut app.state.project_tracks)
                .tracks
                .push(reopened);
        }
        let pending = app.cmd_tx.pending_len();
        assert!(pending > 0);
        let (mut producer, consumer) = rtrb::RingBuffer::new(1);
        app.event_rx = Some(consumer);
        producer
            .push(EngineEvent::DeviceReconfiguration(device))
            .unwrap();
        app.poll_engine_events();
        assert_eq!(restarts.load(Ordering::Relaxed), 0);
        assert_eq!(
            app.state.project_tracks.tracks[0].effects[0].latency_samples,
            Some(777)
        );
        assert_eq!(app.cmd_tx.pending_len(), pending);
    }
}
