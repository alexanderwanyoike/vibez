//! Stopped graph presentation follows the advancing Perform clock without moving Arrange.

use super::*;
use vibez_core::midi::InstrumentKind;
use vibez_core::perform::NoteRepeatRate;
use vibez_core::routing::RoutingChannel;

fn graph_engine(
    channels: &[RoutingChannel],
) -> (AudioEngine, Producer<EngineCommand>, Consumer<EngineEvent>) {
    let (mut engine, commands, events) = AudioEngine::new();
    let plan = crate::routing::PreparedRouting::prepare(channels, 128).unwrap();
    engine.command_set_routing(plan);
    (engine, commands, events)
}

fn routing_channel(id: TrackId) -> RoutingChannel {
    RoutingChannel {
        id,
        is_bus: id.is_master(),
        effects: vec![],
        sends: vec![],
    }
}

#[test]
fn stopped_graph_repeats_keep_source_and_heard_timestamps_across_callback_sizes() {
    for blocks in [vec![50], vec![10, 40], vec![1; 50]] {
        let track = TrackId::new();
        let (mut engine, mut commands, mut events) =
            graph_engine(&[routing_channel(track), routing_channel(TrackId::MASTER)]);
        commands.push(EngineCommand::SetSampleRate(96)).unwrap();
        commands.push(EngineCommand::SetBpm(60.0)).unwrap();
        commands
            .push(EngineCommand::SetProjectSwing(SwingAmount::new(0.62)))
            .unwrap();
        commands.push(EngineCommand::add_track(track)).unwrap();
        commands
            .push(EngineCommand::set_built_in_instrument(
                track,
                InstrumentKind::SubtractiveSynth,
                96,
            ))
            .unwrap();
        engine.process(&mut [0.0; 29 * 2], 2);
        while events.pop().is_ok() {}
        commands
            .push(EngineCommand::StartNoteRepeat {
                id: 0,
                track_id: track,
                pitch: 42,
                velocity: 100,
                rate: NoteRepeatRate::Sixteenth,
            })
            .unwrap();
        for frames in &blocks {
            engine.process(&mut vec![0.0; frames * 2], 2);
        }
        let mut source = vec![];
        let mut heard = vec![];
        while let Ok(event) = events.pop() {
            match event {
                EngineEvent::SourceNoteRepeated { position, .. } => {
                    source.push((position.effective_at_samples, position.canonical_at_samples));
                }
                EngineEvent::NoteRepeated {
                    effective_at_samples,
                    canonical_at_samples,
                    ..
                } => heard.push((effective_at_samples, canonical_at_samples)),
                _ => {}
            }
        }
        assert_eq!(source, [(29, 29), (59, 53), (77, 77)], "blocks {blocks:?}");
        assert_eq!(heard, source, "stopped heard clock, blocks {blocks:?}");
        assert_eq!(engine.transport.position(), 0);
        assert_eq!(engine.performance_position, 79);
    }
}

#[test]
fn stopped_graph_history_advances_perform_without_moving_arrange() {
    let (mut engine, mut commands, _events) = graph_engine(&[routing_channel(TrackId::MASTER)]);
    commands.push(EngineCommand::Seek(100)).unwrap();
    engine.process(&mut [0.0; 29 * 2], 2);
    let presented = engine
        .routing
        .as_ref()
        .presentation
        .audible()
        .expect("last processed sample");
    assert_eq!(engine.transport.position(), 100);
    assert_eq!(engine.performance_position, 129);
    assert_eq!((presented.arrange, presented.perform), (100, 128));
}
