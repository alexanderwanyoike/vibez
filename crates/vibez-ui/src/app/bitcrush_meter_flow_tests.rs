//! Verifies native Bitcrush snapshots stay within the current selected prepared owner lifecycle.

use super::*;
use crate::domains::{devices::DevicesMsg, EngineHandle};
use crate::state::{ProjectTrack, Workspace};
use vibez_core::effect::{EffectType, SampleHoldMeter};

#[test]
fn sample_hold_cache_requires_current_native_selection_and_prepared_context() {
    let mut app = super::super::test_support::app();
    app.state.transport.sample_rate = 48_000;
    app.send_command(EngineCommand::SetSampleRate(48_000));
    app.state.view.workspace = Workspace::Arrange;
    app.state.view.detail_panel_tab = DetailPanelTab::Devices;
    let channel = ProjectTrack::new(TrackId::new(), "Bitcrush".into(), 3);
    let track = channel.id;
    let other = ProjectTrack::new(TrackId::new(), "Other".into(), 6);
    let other_track = other.id;
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .extend([channel, other]);
    let _ = app.update(Message::Devices(DevicesMsg::AddEffect(
        track,
        EffectType::Bitcrush,
    )));
    let effect = app.state.find_track(track).unwrap().effects[0].id;
    app.state.arrangement.selected_track = Some(track);
    app.poll_device_meters();
    assert_eq!(app.device_meters.channel, Some(track));
    let generation = app.device_meters.generation;
    assert_eq!(
        app.cmd_tx.prepared_effect_sample_rate(track, effect),
        Some(48_000)
    );

    let (mut events, receiver) = rtrb::RingBuffer::new(4);
    app.event_rx = Some(receiver);
    let input = MeterValues {
        peak_l: 0.5,
        peak_r: 0.25,
        rms_l: 0.3,
        rms_r: 0.1,
    };
    let output = MeterValues {
        peak_l: 0.25,
        peak_r: 0.125,
        rms_l: 0.2,
        rms_r: 0.05,
    };
    events
        .push(EngineEvent::EffectMeter {
            track_id: track,
            effect_id: effect,
            generation,
            input,
            output,
        })
        .unwrap();
    app.poll_engine_events();
    assert_eq!(app.device_meters.readings[&effect].input, input);
    assert_eq!(app.device_meters.readings[&effect].output, output);

    let mut packet = SampleHoldMeter {
        count: 8,
        sample_rate: 48_000.0,
        first_source_frame: 2048,
        primary_interval_frames: 4,
        secondary_interval_frames: 2,
        transition_active: true,
        primary_acquisitions: [0x11, 0],
        secondary_acquisitions: [0x55, 0],
        blending: [0xff, 0],
        ..Default::default()
    };
    packet.input[..8].copy_from_slice(&[0.0, 0.25, 0.5, 0.75, 0.5, 0.25, 0.0, -0.25]);
    packet.output[..8].copy_from_slice(&[0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.25, 0.25]);
    let before = app.state.find_track(track).unwrap().effects[0]
        .params
        .clone();
    app.state.project.history = Default::default();
    app.state.project.dirty = false;
    for (candidate_track, candidate_effect, candidate_generation) in [
        (other_track, effect, generation),
        (track, EffectId::new(), generation),
        (track, effect, generation.wrapping_sub(1)),
    ] {
        app.accept_bitcrush_sample_hold(
            candidate_track,
            candidate_effect,
            candidate_generation,
            packet,
        );
        assert!(app.bitcrush_sample_hold(effect).is_none());
    }
    app.accept_bitcrush_sample_hold(track, effect, generation, packet);
    assert_eq!(app.bitcrush_sample_hold(effect), Some(packet));
    let silence = SampleHoldMeter {
        input: [0.0; 128],
        output: [0.0; 128],
        first_source_frame: 4096,
        ..packet
    };
    app.accept_bitcrush_sample_hold(track, effect, generation, silence);
    assert_eq!(app.bitcrush_sample_hold(effect), Some(silence));
    app.accept_bitcrush_sample_hold(track, effect, generation, SampleHoldMeter::default());
    assert!(app.bitcrush_sample_hold(effect).is_none());

    app.accept_bitcrush_sample_hold(track, effect, generation, packet);
    app.poll_bitcrush_panels(Some(track), Instant::now() + Duration::from_secs(1));
    assert!(app.bitcrush_sample_hold(effect).is_none());
    app.accept_bitcrush_sample_hold(track, effect, generation, packet);
    app.state.find_track_mut(track).unwrap().effects[0].bypass = true;
    app.poll_device_meters();
    assert!(app.bitcrush_sample_hold(effect).is_none());
    app.accept_bitcrush_sample_hold(track, effect, generation, packet);
    assert!(app.bitcrush_sample_hold(effect).is_none());
    app.state.find_track_mut(track).unwrap().effects[0].bypass = false;

    for marker in ["name", "reference", "type"] {
        let generation = app.device_meters.generation;
        app.accept_bitcrush_sample_hold(track, effect, generation, packet);
        assert_eq!(app.bitcrush_sample_hold(effect), Some(packet));
        let target = &mut app.state.find_track_mut(track).unwrap().effects[0];
        match marker {
            "name" => target.plugin_name = Some("Hosted".into()),
            "reference" => {
                target.plugin_ref = Some(vibez_core::effect::PluginDeviceInfo {
                    format: "clap".into(),
                    uid: "hosted-bitcrush-fallback".into(),
                    path: std::path::PathBuf::new(),
                    name: "Hosted".into(),
                    state_b64: None,
                });
            }
            "type" => target.effect_type = EffectType::Filter,
            _ => unreachable!(),
        }
        app.poll_device_meters();
        app.accept_bitcrush_sample_hold(track, effect, app.device_meters.generation, packet);
        assert!(app.bitcrush_sample_hold(effect).is_none());
        let target = &mut app.state.find_track_mut(track).unwrap().effects[0];
        target.plugin_name = None;
        target.plugin_ref = None;
        target.effect_type = EffectType::Bitcrush;
        app.poll_device_meters();
    }
    let generation = app.device_meters.generation;
    app.accept_bitcrush_sample_hold(track, effect, generation, packet);
    app.state.arrangement.selected_track = Some(other_track);
    app.poll_device_meters();
    assert!(app.bitcrush_sample_hold(effect).is_none());
    app.accept_bitcrush_sample_hold(track, effect, generation, packet);
    assert!(app.bitcrush_sample_hold(effect).is_none());
    app.state.arrangement.selected_track = Some(track);
    app.poll_device_meters();
    app.accept_bitcrush_sample_hold(track, effect, generation, packet);
    assert!(app.bitcrush_sample_hold(effect).is_none());
    let generation = app.device_meters.generation;
    app.accept_bitcrush_sample_hold(track, effect, generation, packet);
    assert_eq!(app.bitcrush_sample_hold(effect), Some(packet));

    app.send_command(EngineCommand::SetSampleRate(96_000));
    app.poll_device_meters();
    app.accept_bitcrush_sample_hold(track, effect, generation, packet);
    assert!(app.bitcrush_sample_hold(effect).is_none());
    assert_eq!(app.cmd_tx.prepared_effect_sample_rate(track, effect), None);
    assert!(app
        .device_parameter_presentation(track, effect, vibez_dsp::bitcrush::param::SR_REDUCE)
        .unwrap()
        .value
        .is_none());
    app.cmd_tx
        .insert_built_in_effect_with_params(track, effect, EffectType::Bitcrush, &before);
    app.accept_bitcrush_sample_hold(track, effect, generation, packet);
    assert!(app.bitcrush_sample_hold(effect).is_none());
    let replacement = SampleHoldMeter {
        sample_rate: 96_000.0,
        first_source_frame: 8192,
        ..packet
    };
    app.accept_bitcrush_sample_hold(track, effect, generation, replacement);
    assert_eq!(app.bitcrush_sample_hold(effect), Some(replacement));
    app.state.find_track_mut(track).unwrap().effects[0].reconfiguration_failed = true;
    app.poll_device_meters();
    app.accept_bitcrush_sample_hold(track, effect, generation, replacement);
    assert!(app.bitcrush_sample_hold(effect).is_none());
    app.state.find_track_mut(track).unwrap().effects[0].reconfiguration_failed = false;
    app.accept_bitcrush_sample_hold(track, effect, generation, replacement);
    assert_eq!(app.bitcrush_sample_hold(effect), Some(replacement));
    assert_eq!(
        app.state.find_track(track).unwrap().effects[0].params,
        before
    );
    assert!(!app.state.project.history.can_undo());
    assert!(!app.state.project.dirty);

    app.reset_device_meters();
    app.accept_bitcrush_sample_hold(track, effect, generation, replacement);
    assert!(app.bitcrush_sample_hold(effect).is_none());
    app.poll_device_meters();
    let generation = app.device_meters.generation;
    app.accept_bitcrush_sample_hold(track, effect, generation, replacement);
    assert_eq!(app.bitcrush_sample_hold(effect), Some(replacement));
    let _ = app.update(Message::Devices(DevicesMsg::RemoveEffect(track, effect)));
    app.poll_device_meters();
    app.accept_bitcrush_sample_hold(track, effect, generation, replacement);
    assert!(app.bitcrush_sample_hold(effect).is_none());
}
