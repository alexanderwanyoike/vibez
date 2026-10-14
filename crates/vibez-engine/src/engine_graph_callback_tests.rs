//! First and steady callbacks keep Arrange, Perform and monitoring within prepared stereo storage.

use super::*;
use crate::playback_source::{EngineClip, PreparedPlaybackSource, PreparedSectionPlaybackSource};
use graph_test_support::{graph_engine, routing_channel};
use vibez_core::id::ClipId;
use vibez_core::routing::RoutingChannel;

#[derive(Debug, Clone, Copy)]
enum SourceMode {
    Arrange,
    Section,
    Monitor,
}

fn source_clip() -> EngineClip {
    EngineClip {
        id: ClipId::new(),
        audio: Arc::new(DecodedAudio {
            channels: vec![vec![0.25; 16_384]; 2],
            sample_rate: DEFAULT_SAMPLE_RATE,
        }),
        position: 0,
        source_offset: 0,
        start_marker: 0,
        duration: 16_384,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 0,
        linear_gain: 1.0,
        fades: Default::default(),
        playback_direction: Default::default(),
        warp_markers: Default::default(),
    }
}

#[test]
fn graph_callbacks_render_each_clock_domain_without_memory_operations_at_hardware_widths() {
    for mode in [
        SourceMode::Arrange,
        SourceMode::Section,
        SourceMode::Monitor,
    ] {
        for channels in [1, 2, 4, 6, 16] {
            for frames in [64, crate::routing::MAX_BLOCK_FRAMES + 1] {
                let track = TrackId::new();
                let bus = TrackId::new();
                let model = [
                    RoutingChannel {
                        sends: vec![bus],
                        ..routing_channel(track)
                    },
                    RoutingChannel {
                        is_bus: true,
                        ..routing_channel(bus)
                    },
                    routing_channel(TrackId::MASTER),
                ];
                let (mut engine, mut commands, mut events) = graph_engine(&model);
                commands.push(EngineCommand::add_track(track)).unwrap();
                commands.push(EngineCommand::add_bus(bus)).unwrap();
                commands
                    .push(EngineCommand::SetSend {
                        track_id: track,
                        bus_id: bus,
                        amount: 1.0,
                    })
                    .unwrap();
                commands
                    .push(EngineCommand::SetTrackGain(bus, 0.5))
                    .unwrap();
                match mode {
                    SourceMode::Arrange => {
                        let clip = source_clip();
                        commands
                            .push(EngineCommand::AddClip {
                                track_id: track,
                                clip_id: clip.id,
                                audio: clip.audio,
                                position: clip.position,
                                source_offset: clip.source_offset,
                                start_marker: clip.start_marker,
                                duration: clip.duration,
                                loop_enabled: clip.loop_enabled,
                                loop_start: clip.loop_start,
                                loop_end: clip.loop_end,
                                linear_gain: clip.linear_gain,
                                fades: clip.fades,
                                playback_direction: clip.playback_direction,
                                warp_markers: clip.warp_markers,
                            })
                            .unwrap();
                        commands.push(EngineCommand::Play).unwrap();
                    }
                    SourceMode::Section => commands
                        .push(EngineCommand::LaunchSection(Box::new(
                            PreparedSectionPlaybackSource::new(
                                SectionId::new(),
                                4.0,
                                true,
                                vec![(
                                    track,
                                    PreparedPlaybackSource::new(
                                        vec![source_clip()],
                                        vec![],
                                        vec![],
                                    ),
                                )],
                            ),
                        )))
                        .unwrap(),
                    SourceMode::Monitor => {}
                }
                let input = vec![0.25; frames * channels];
                let mut output = vec![0.0; frames * channels];
                let mut capture = vec![0.0; frames * channels];
                for callback in 0..2 {
                    assert_eq!(
                        crate::retirement::tests::allocations(|| {
                            let mut block = AudioProcessBlock::new(&mut output, channels)
                                .with_track_output_capture(track.raw(), &mut capture);
                            if matches!(mode, SourceMode::Monitor) {
                                block = block.with_live_input(track.raw(), &input);
                            }
                            engine.process_block(block);
                        }),
                        (0, 0),
                        "{mode:?}, channels={channels}, frames={frames}, callback={callback}"
                    );
                    let pan = if channels == 1 {
                        1.0
                    } else {
                        std::f32::consts::FRAC_1_SQRT_2
                    };
                    for (mixed, selected) in output
                        .chunks_exact(channels)
                        .zip(capture.chunks_exact(channels))
                    {
                        for channel in 0..channels {
                            let dry = if channel < 2 { 0.25 * pan } else { 0.0 };
                            assert!((mixed[channel] - dry * 1.5).abs() < 1e-6,
                                "{mode:?}, channels={channels}, frames={frames}, callback={callback}, channel={channel}: {}", mixed[channel]);
                            assert!((selected[channel] - dry).abs() < 1e-6);
                        }
                    }
                    while events.pop().is_ok() {}
                }
            }
        }
    }
}
