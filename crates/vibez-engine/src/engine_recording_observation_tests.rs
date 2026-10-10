//! Actual engine paths retain source coordinates beside unchanged monitor notices.

use super::*;
use crate::events::SourceRecordingPosition;
use crate::playback_source::PreparedSectionPlaybackSource;
use vibez_core::{midi::InstrumentKind, perform::NoteRepeatRate, routing::*};

#[test]
fn external_input_observations_keep_the_applied_section_coordinate() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let section = SectionId::new();
    engine.tracks.push(EngineTrack::new(track));
    engine.process(&mut [0.0; 8], 1);
    commands
        .push(EngineCommand::LaunchSection(Box::new(
            PreparedSectionPlaybackSource::new(section, 8.0, true, vec![]),
        )))
        .unwrap();
    engine.process(&mut [0.0; 4], 1);
    while events.pop().is_ok() {}
    for on in [true, false] {
        let position = engine.performance_position;
        let local = engine.active_section.unwrap().position_samples;
        commands
            .push(if on {
                EngineCommand::ExternalNoteOn {
                    track_id: track,
                    pitch: 42,
                    velocity: 100,
                }
            } else {
                EngineCommand::ExternalNoteOff {
                    track_id: track,
                    pitch: 42,
                }
            })
            .unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 1], 1)),
            (0, 0)
        );
        let observed: Vec<_> = std::iter::from_fn(|| events.pop().ok())
            .filter(|event| {
                matches!(
                    event,
                    EngineEvent::SourceNoteInput { .. } | EngineEvent::InstrumentNoteInput { .. }
                )
            })
            .collect();
        assert_eq!(
            observed,
            [
                EngineEvent::SourceNoteInput {
                    track_id: track,
                    pitch: 42,
                    velocity: if on { 100 } else { 0 },
                    on,
                    position: SourceRecordingPosition {
                        effective_at_samples: position,
                        canonical_at_samples: position,
                        section_id: Some(section),
                        section_position_samples: Some(local),
                        canonical_section_position_samples: Some(local)
                    }
                },
                EngineEvent::InstrumentNoteInput {
                    track_id: track,
                    pitch: 42,
                    velocity: if on { 100 } else { 0 },
                    on,
                    effective_at_samples: position,
                    section_id: Some(section),
                    section_position_samples: Some(local)
                }
            ]
        );
    }
}

#[test]
fn repeats_observe_the_same_coordinates_in_graph_legacy_and_stopped_paths() {
    for (graph, playing) in [(true, true), (false, true), (false, false)] {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        engine.sample_rate = 128;
        engine.transport.set_bpm(60.0);
        let id = TrackId::new();
        let mut track = EngineTrack::new(id);
        track.instrument = Some(create_instrument(InstrumentKind::SubtractiveSynth, 128.0));
        engine.tracks.push(track);
        engine.process(&mut [0.0; 256], 2);
        if graph {
            let channels = [
                RoutingChannel {
                    id,
                    is_bus: false,
                    sends: vec![],
                    effects: vec![],
                },
                RoutingChannel {
                    id: TrackId::MASTER,
                    is_bus: true,
                    sends: vec![],
                    effects: vec![],
                },
            ];
            engine.routing =
                Some(crate::routing::PreparedRouting::prepare(&channels, 128).unwrap());
        }
        if playing {
            commands.push(EngineCommand::Play).unwrap();
        }
        commands
            .push(EngineCommand::StartNoteRepeat {
                id: 1,
                track_id: id,
                pitch: 42,
                velocity: 100,
                rate: NoteRepeatRate::Eighth,
            })
            .unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 256], 2)),
            (0, 0)
        );
        let mut source = Vec::new();
        let mut notices = Vec::new();
        while let Ok(event) = events.pop() {
            match event {
                EngineEvent::SourceNoteRepeated { position, .. } => source.push(position),
                EngineEvent::NoteRepeated {
                    effective_at_samples,
                    canonical_at_samples,
                    section_id,
                    section_position_samples,
                    canonical_section_position_samples,
                    ..
                } => notices.push(SourceRecordingPosition {
                    effective_at_samples,
                    canonical_at_samples,
                    section_id,
                    section_position_samples,
                    canonical_section_position_samples,
                }),
                _ => {}
            }
        }
        assert!(!source.is_empty(), "graph={graph}, playing={playing}");
        assert_eq!(source, notices);
    }
}
