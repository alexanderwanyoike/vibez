//! Exercises queued applied observations against real App edits and bounded command publication.

use super::*;
use crate::domains::{devices::DevicesMsg, EngineCommandQueue};
use crate::state::{DetailPanelTab, ProjectTrack};
use vibez_core::effect::EffectType;
use vibez_dsp::delay::param;
use vibez_engine::engine::{AudioEngine, AudioProcessBlock};

fn publish_all(
    app: &mut App,
    published: &mut rtrb::Consumer<EngineCommand>,
    engine_commands: &mut rtrb::Producer<EngineCommand>,
) {
    loop {
        app.cmd_tx.flush();
        let Ok(command) = published.pop() else {
            assert_eq!(app.cmd_tx.pending_len(), 0);
            break;
        };
        engine_commands.push(command).unwrap();
    }
}

fn render_window(engine: &mut AudioEngine, track: TrackId) {
    let silence = [0.0; 40];
    let mut output = [0.0; 40];
    engine.process_block(
        AudioProcessBlock::new(&mut output, 2).with_live_input(track.raw(), &silence),
    );
    assert!(output.iter().all(|&sample| sample == 0.0));
}

#[test]
fn pre_edit_observation_cannot_acknowledge_a_revert_while_its_intermediate_edit_is_still_queued() {
    let (mut engine, mut commands, events) = AudioEngine::new();
    let (producer, mut published) = rtrb::RingBuffer::new(1);
    let mut app = super::super::test_support::app();
    app.cmd_tx = EngineCommandQueue::new(producer);
    app.event_rx = Some(events);
    app.state.transport.sample_rate = 1000;
    app.send_command(EngineCommand::SetSampleRate(1000));
    let track = ProjectTrack::new(TrackId::new(), "Queued Delay".into(), 3);
    let track_id = track.id;
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(track);
    app.send_command(EngineCommand::add_track(track_id));
    let _ = app.update(Message::Devices(DevicesMsg::AddEffect(
        track_id,
        EffectType::Delay,
    )));
    let effect_id = app.state.find_track(track_id).unwrap().effects[0].id;
    app.state.arrangement.selected_track = Some(track_id);
    app.state.view.detail_panel_tab = DetailPanelTab::Devices;
    app.poll_device_meters();
    publish_all(&mut app, &mut published, &mut commands);
    render_window(&mut engine, track_id);
    app.poll_engine_events();
    let first = app.delay_devices.panels[&effect_id].snapshot.unwrap();
    let original = first.targets[param::FEEDBACK];
    assert_eq!(original, 0.35);
    assert!(!first.reference_moving);

    // This genuine observation is already in the event ring before either edit exists.
    render_window(&mut engine, track_id);
    let queued = engine.tracks()[0].effects[0]
        .effect
        .delay_snapshot()
        .unwrap();
    assert!(queued.observation_order > first.observation_order);
    assert_eq!(
        queued.targets[param::FEEDBACK].to_bits(),
        original.to_bits()
    );
    let _ = app.update(Message::set_effect_param(
        track_id,
        effect_id,
        param::FEEDBACK,
        0.7,
    ));
    let _ = app.update(Message::set_effect_param(
        track_id,
        effect_id,
        param::FEEDBACK,
        original,
    ));
    assert!(
        app.cmd_tx.pending_len() >= 1,
        "capacity1 separates the original edit from its revert"
    );
    assert_eq!(
        app.delay_reference(track_id, effect_id).1,
        "Awaiting applied state"
    );
    app.poll_engine_events();
    let old_observation_kept_pending =
        app.delay_reference(track_id, effect_id).1 == "Awaiting applied state";

    let command = published.pop().unwrap();
    assert!(matches!(&command, EngineCommand::SetEffectParam {
        track_id: track, effect_id: effect, param_index: param::FEEDBACK, value,
    } if *track == track_id && *effect == effect_id && *value == 0.7));
    commands.push(command).unwrap();
    render_window(&mut engine, track_id);
    assert_eq!(
        engine.tracks()[0].effects[0]
            .effect
            .get_param(param::FEEDBACK),
        0.7
    );
    assert_eq!(
        app.state.find_track(track_id).unwrap().effects[0].params[param::FEEDBACK],
        original
    );
    app.poll_engine_events();
    let intermediate_kept_pending =
        app.delay_reference(track_id, effect_id).1 == "Awaiting applied state";

    publish_all(&mut app, &mut published, &mut commands);
    render_window(&mut engine, track_id);
    app.poll_engine_events();
    let applied = app.delay_devices.panels[&effect_id].snapshot.unwrap();
    assert_eq!(
        applied.targets[param::FEEDBACK].to_bits(),
        original.to_bits()
    );
    assert!(applied.observation_order > queued.observation_order);
    assert_ne!(
        app.delay_reference(track_id, effect_id).1,
        "Awaiting applied state"
    );
    assert!(old_observation_kept_pending && intermediate_kept_pending,
        "queued old snapshot acknowledged revert before execution: old_kept_pending={old_observation_kept_pending}, intermediate_kept_pending={intermediate_kept_pending}");
}
