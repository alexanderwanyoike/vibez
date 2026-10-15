//! Every engine command either drains without allocating in the callback or is
//! a named allocation still to remove, so the allocating set can only shrink.

use super::*;
use crate::playback_source::{
    PreparedClipPlayback, PreparedPlaybackSource, PreparedSectionPlaybackSource,
};
use graph_test_support::{graph_engine, routing_channel, routing_effect, routing_plan};
use vibez_core::automation::{AutomationLane, AutomationTarget};
use vibez_core::id::{ClipId, EffectId, LaneId, SectionId, TrackId};
use vibez_core::midi::{InstrumentKind, MidiNote};
use vibez_core::perform::{NoteRepeatRate, SectionLaunchQuantization};

/// Where a command's callback allocation is measured.
enum Coverage {
    Table,
    /// Prepared-owner commands whose allocation-free drain is proven by the
    /// named test, because their owners need a full published plan.
    Elsewhere(&'static str),
}

// No wildcard arm: a new command cannot compile without declaring coverage.
fn coverage(command: &EngineCommand) -> Coverage {
    use EngineCommand as C;
    match command {
        C::SetRouting(_) => Coverage::Elsewhere(
            "graph_tests::hardware_first_pair_live_capture_and_populated_graph_are_allocation_free",
        ),
        C::UpdateAutomationRouting(_) => Coverage::Elsewhere(
            "transport_timing_review_tests::unchanged_alignment_publication_preserves_a_varying_waveform_sample_for_sample",
        ),
        C::ResumeDeviceReconfiguration { .. } => Coverage::Elsewhere(
            "recovery_review_tests::parameter_edits_while_main_holds_the_owner_are_coalesced_and_restored_without_allocation",
        ),
        C::RejectDeviceReconfiguration { .. } => Coverage::Elsewhere(
            "recovery_review_tests::reject_worst_case_retirement_is_reserved_before_command_ownership_is_popped",
        ),
        C::LaunchSection(_) => Coverage::Elsewhere(
            "transport_timing_review_tests::first_section_launch_preserves_unaffected_alignment_history",
        ),
        C::QueueClips { .. } => Coverage::Elsewhere(
            "clip_batch_tests::partial_packet_releases_tracks_atomically_at_one_source_timestamp_and_keeps_other_clips_running",
        ),
        C::RejectRoutingUpdate { .. }
        | C::Play
        | C::Stop
        | C::Seek(_)
        | C::SetBpm(_)
        | C::SetProjectSwing(_)
        | C::BeginClipPerformance
        | C::ArmClipRecord { .. }
        | C::StopClipRecord { .. }
        | C::RefreshClip(_)
        | C::EditClip { .. }
        | C::QueueSection { .. }
        | C::RefreshSection(_)
        | C::ArmSectionRecord { .. }
        | C::StopSectionRecord
        | C::StartPerformanceCapture
        | C::StopPerformanceCapture
        | C::UnloadAudio
        | C::AddTrack(..)
        | C::RemoveTrack(_)
        | C::ReorderTracks(_)
        | C::AddClip { .. }
        | C::RemoveClip(..)
        | C::ReplaceClipAudio { .. }
        | C::ReplaceClipBuffer { .. }
        | C::MoveClip { .. }
        | C::SetClipGain { .. }
        | C::SetClipFades { .. }
        | C::SetClipPlaybackDirection { .. }
        | C::SetClipWarpMarkers { .. }
        | C::SetClipBounds { .. }
        | C::SetTrackGain(..)
        | C::SetAutomationLane { .. }
        | C::RemoveAutomationLane { .. }
        | C::SetTrackPan(..)
        | C::SetTrackMute(..)
        | C::QueueTrackMute { .. }
        | C::SetAutomationOverride { .. }
        | C::UpdateAutomationGesture { .. }
        | C::EndAutomationGesture { .. }
        | C::SetTrackSolo(..)
        | C::SetTrackSwingOffset(..)
        | C::AddBus(..)
        | C::RemoveBus(_)
        | C::SetSend { .. }
        | C::SetSampleRate(_)
        | C::SetSpectrumTap(_)
        | C::RemoveEffect(..)
        | C::SetEffectParam { .. }
        | C::SetEffectBypass { .. }
        | C::MoveEffect { .. }
        | C::RemoveTrackInstrument(_)
        | C::SetNoteClipDuration { .. }
        | C::SetNoteClipGrooveGrid { .. }
        | C::AddNoteClip { .. }
        | C::RemoveNoteClip(..)
        | C::MoveNoteClip { .. }
        | C::AddNote { .. }
        | C::RemoveNote { .. }
        | C::EditNote { .. }
        | C::SetInstrumentParam { .. }
        | C::LoadSamplerSample { .. }
        | C::LoadDrumRackPadSample { .. }
        | C::ClearDrumRackPad { .. }
        | C::SetDrumRackPadState { .. }
        | C::SetArrangementRecording(_)
        | C::SetArrangementLoop(_)
        | C::SetArrangementLoopRegion { .. }
        | C::SetClipLoop { .. }
        | C::SetClipStartMarker { .. }
        | C::SetNoteClipLoop { .. }
        | C::SetNoteClipStartMarker { .. }
        | C::StartAudition { .. }
        | C::StopAudition
        | C::SetAuditionGain(_)
        | C::ExternalNoteOn { .. }
        | C::ExternalNoteOff { .. }
        | C::StartNoteRepeat { .. }
        | C::UpdateNoteRepeatRate { .. }
        | C::StopNoteRepeat { .. }
        | C::InsertEffect { .. }
        | C::AuditionNote { .. }
        | C::SetInstrument { .. } => Coverage::Table,
    }
}

/// A playing project with one of everything a command can address, plus empty
/// targets so insertions meet a collection at its capacity boundary.
struct Fixture {
    synth: TrackId,
    drums: TrackId,
    audio: TrackId,
    empty: TrackId,
    empty_audio: TrackId,
    bus: TrackId,
    effect: EffectId,
    clip: ClipId,
    note_clip: ClipId,
    empty_note_clip: ClipId,
    lane: LaneId,
}

fn audio(frames: usize) -> Arc<DecodedAudio> {
    Arc::new(DecodedAudio {
        channels: vec![vec![0.25; frames]],
        sample_rate: 48_000,
    })
}

fn note() -> MidiNote {
    MidiNote {
        pitch: 60,
        velocity: 100,
        start_beat: 0.0,
        duration_beats: 1.0,
    }
}

fn nonempty_warp_markers() -> vibez_core::warp_marker::WarpMarkers {
    let mut markers = vibez_core::warp_marker::WarpMarkers::default();
    assert!(markers.add(1024, 2048, 0, 4096, 4096));
    markers
}

fn add_clip(track_id: TrackId, clip_id: ClipId) -> EngineCommand {
    EngineCommand::AddClip {
        insertion: crate::clip_insertion::PreparedClipInsertion::new(),
        track_id,
        clip_id,
        audio: audio(4096),
        position: 0,
        source_offset: 0,
        start_marker: 0,
        duration: 4096,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 0,
        linear_gain: 1.0,
        fades: Default::default(),
        playback_direction: Default::default(),
        warp_markers: Default::default(),
    }
}

fn add_note_clip(track_id: TrackId, clip_id: ClipId) -> EngineCommand {
    EngineCommand::AddNoteClip {
        track_id,
        insertion: crate::clip_insertion::PreparedNoteClipInsertion::new(
            clip_id,
            0.0,
            4.0,
            0.0,
            false,
            0.0,
            4.0,
            Default::default(),
        ),
    }
}

fn section(fixture: &Fixture) -> Box<PreparedSectionPlaybackSource> {
    Box::new(PreparedSectionPlaybackSource::new(
        SectionId::new(),
        4.0,
        true,
        vec![(
            fixture.synth,
            PreparedPlaybackSource::new(Vec::new(), Vec::new(), Vec::new()),
        )],
    ))
}

fn playing(
    setup: impl FnOnce(&mut rtrb::Producer<EngineCommand>, &Fixture),
) -> (
    AudioEngine,
    rtrb::Producer<EngineCommand>,
    rtrb::Consumer<EngineEvent>,
    Fixture,
) {
    let fixture = Fixture {
        synth: TrackId::new(),
        drums: TrackId::new(),
        audio: TrackId::new(),
        empty: TrackId::new(),
        empty_audio: TrackId::new(),
        bus: TrackId::new(),
        effect: EffectId::new(),
        clip: ClipId::new(),
        note_clip: ClipId::new(),
        empty_note_clip: ClipId::new(),
        lane: LaneId::new(),
    };
    let f = &fixture;
    let mut synth = routing_channel(f.synth);
    synth.effects.push(routing_effect(f.effect));
    synth.sends.push(f.bus);
    let mut bus = routing_channel(f.bus);
    bus.is_bus = true;
    let channels = [
        synth,
        routing_channel(f.drums),
        routing_channel(f.audio),
        routing_channel(f.empty),
        routing_channel(f.empty_audio),
        bus,
        routing_channel(TrackId::MASTER),
    ];
    let (engine, mut commands, events) = graph_engine(&channels);
    let mut lane = AutomationLane::new(AutomationTarget::TrackPan);
    lane.id = f.lane;
    for command in [
        EngineCommand::SetSampleRate(48_000),
        EngineCommand::add_track(f.synth),
        EngineCommand::set_built_in_instrument(f.synth, InstrumentKind::SubtractiveSynth, 48_000),
        EngineCommand::add_track(f.drums),
        EngineCommand::set_built_in_instrument(f.drums, InstrumentKind::DrumRack, 48_000),
        EngineCommand::add_track(f.audio),
        EngineCommand::add_track(f.empty),
        EngineCommand::add_track(f.empty_audio),
        EngineCommand::add_bus(f.bus),
        EngineCommand::insert_built_in_effect(
            f.synth,
            f.effect,
            vibez_core::effect::EffectType::Gain,
            48_000,
        ),
        add_clip(f.audio, f.clip),
        add_note_clip(f.synth, f.note_clip),
        EngineCommand::AddNote {
            track_id: f.synth,
            clip_id: f.note_clip,
            insertion: crate::clip_insertion::PreparedNoteInsertion::new(note()),
        },
        add_note_clip(f.synth, f.empty_note_clip),
        EngineCommand::SetSend {
            track_id: f.synth,
            bus_id: f.bus,
            amount: 0.5,
        },
        EngineCommand::SetAutomationLane {
            track_id: f.synth,
            insertion: crate::clip_insertion::PreparedAutomationLaneInsertion::new(lane),
        },
    ] {
        assert!(commands.push(command).is_ok());
    }
    setup(&mut commands, f);
    assert!(commands
        .push(EngineCommand::SetRouting(routing_plan(&channels)))
        .is_ok());
    assert!(commands.push(EngineCommand::Play).is_ok());
    let mut engine = engine;
    let mut events = events;
    let mut output = [0.0_f32; 512];
    for _ in 0..4 {
        engine.process(&mut output, 2);
        while let Ok(event) = events.pop() {
            if let EngineEvent::ClipInsertionStorageRequested(request) = event {
                request.fulfill();
            }
        }
    }
    assert!(engine.pending_clip_insertion.is_none());
    assert_eq!(
        commands.slots(),
        vibez_core::constants::RING_BUFFER_CAPACITY
    );
    assert!(engine.transport().is_playing());
    let synth = engine
        .tracks()
        .iter()
        .find(|track| track.id == f.synth)
        .unwrap();
    let clips = &synth.playback_source.note_clips;
    assert_eq!(clips.len(), 2);
    assert_eq!([clips[0].id, clips[1].id], [f.note_clip, f.empty_note_clip]);
    assert_eq!(clips[0].notes, [note()]);
    assert!(clips[1].notes.is_empty());
    assert_eq!(clips[1].notes.capacity(), 0);
    (engine, commands, events, fixture)
}

struct Case {
    name: &'static str,
    /// Card 04 work item: this command still allocates or frees in the callback.
    allocates: bool,
    setup: fn(&mut rtrb::Producer<EngineCommand>, &Fixture),
    command: fn(&Fixture) -> EngineCommand,
}

fn no_setup(_: &mut rtrb::Producer<EngineCommand>, _: &Fixture) {}

fn prepare_populated_sampler_replacement(
    commands: &mut rtrb::Producer<EngineCommand>,
    fixture: &Fixture,
) -> std::sync::Weak<DecodedAudio> {
    let sample = audio(8192);
    let owner = Arc::downgrade(&sample);
    for command in [
        EngineCommand::set_built_in_instrument(fixture.empty, InstrumentKind::Sampler, 48_000),
        EngineCommand::SetInstrumentParam {
            track_id: fixture.empty,
            param_index: 9,
            value: 0.5,
        },
        EngineCommand::LoadSamplerSample {
            track_id: fixture.empty,
            sample,
            sample_name: "Populated original sample".into(),
        },
        EngineCommand::ExternalNoteOn {
            track_id: fixture.empty,
            pitch: 60,
            velocity: 127,
        },
    ] {
        commands.push(command).unwrap();
    }
    owner
}

fn fulfill_clip_insertions(
    engine: &mut AudioEngine,
    events: &mut rtrb::Consumer<EngineEvent>,
) -> (usize, usize) {
    let mut total = crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(&mut [], 2));
    });
    while engine.pending_clip_insertion.is_some() {
        let waiting = crate::retirement::tests::allocations(|| {
            engine.process_block(AudioProcessBlock::new(&mut [], 2));
        });
        total.0 += waiting.0;
        total.1 += waiting.1;
        let mut fulfilled = false;
        while let Ok(event) = events.pop() {
            if let EngineEvent::ClipInsertionStorageRequested(request) = event {
                request.fulfill();
                fulfilled = true;
            }
        }
        assert!(
            fulfilled,
            "the real request must reach its off-callback consumer"
        );
        let completion = crate::retirement::tests::allocations(|| {
            engine.process_block(AudioProcessBlock::new(&mut [], 2));
        });
        total.0 += completion.0;
        total.1 += completion.1;
    }
    total
}

macro_rules! case {
    ($name:literal, $allocates:expr, $command:expr) => {
        Case {
            name: $name,
            allocates: $allocates,
            setup: no_setup,
            command: $command,
        }
    };
    ($name:literal, $allocates:expr, $setup:expr, $command:expr) => {
        Case {
            name: $name,
            allocates: $allocates,
            setup: $setup,
            command: $command,
        }
    };
}

fn cases() -> Vec<Case> {
    use EngineCommand as C;
    vec![
        case!("RejectRoutingUpdate", false, |_| C::RejectRoutingUpdate {
            reason: "test".into()
        }),
        case!("Play", false, |_| C::Play),
        case!("Stop", false, |_| C::Stop),
        case!("Seek", false, |_| C::Seek(64)),
        case!("SetBpm", false, |_| C::SetBpm(128.0)),
        case!("SetProjectSwing", false, |_| C::SetProjectSwing(
            Default::default()
        )),
        case!("BeginClipPerformance", false, |_| C::BeginClipPerformance),
        case!("ArmClipRecord", false, |f| C::ArmClipRecord {
            free_length: true,
            prepared: PreparedClipPlayback::stop(f.synth, 1),
            count_in_bars: 0,
        }),
        case!("StopClipRecord", false, |_| C::StopClipRecord {
            immediate: true
        }),
        case!("RefreshClip", false, |f| C::RefreshClip(
            PreparedClipPlayback::stop(f.synth, 1)
        )),
        case!("EditClip", false, |f| C::EditClip {
            active: PreparedClipPlayback::stop(f.synth, 1),
            queued: PreparedClipPlayback::stop(f.synth, 2),
        }),
        case!("QueueSection", false, |f| C::QueueSection {
            prepared: section(f),
            quantization: SectionLaunchQuantization::OneBar,
        }),
        case!("RefreshSection", false, |f| C::RefreshSection(section(f))),
        case!("ArmSectionRecord", false, |f| C::ArmSectionRecord {
            section_id: SectionId::new(),
            track_id: f.synth,
            prepared: Some(section(f)),
            count_in_bars: 0,
            replace_existing: false,
        }),
        case!("StopSectionRecord", false, |_| C::StopSectionRecord),
        case!("StartPerformanceCapture", false, |_| {
            C::StartPerformanceCapture
        }),
        case!("StopPerformanceCapture", false, |_| {
            C::StopPerformanceCapture
        }),
        case!("UnloadAudio", false, |_| C::UnloadAudio),
        case!("AddTrack", false, |_| C::add_track(TrackId::new())),
        case!("RemoveTrack", false, |f| C::RemoveTrack(f.audio)),
        case!("ReorderTracks", false, |f| C::ReorderTracks(vec![
            f.audio,
            f.synth,
            f.drums,
            f.empty,
            f.empty_audio
        ])),
        case!("AddClip", false, |f| add_clip(f.empty_audio, ClipId::new())),
        case!("RemoveClip", false, |f| C::RemoveClip(f.audio, f.clip)),
        case!("ReplaceClipAudio", false, |f| C::ReplaceClipAudio {
            track_id: f.audio,
            clip_id: f.clip,
            audio: audio(2048),
            duration: 2048,
            source_offset: 0,
            start_marker: 0,
            loop_start: 0,
            loop_end: 0,
        }),
        case!("ReplaceClipBuffer", false, |f| C::ReplaceClipBuffer {
            track_id: f.audio,
            clip_id: f.clip,
            audio: audio(4096),
        }),
        case!("MoveClip", false, |f| C::MoveClip {
            track_id: f.audio,
            clip_id: f.clip,
            new_position: 128,
        }),
        case!("SetClipGain", false, |f| C::SetClipGain {
            track_id: f.audio,
            clip_id: f.clip,
            linear_gain: 0.5,
        }),
        case!("SetClipFades", false, |f| C::SetClipFades {
            track_id: f.audio,
            clip_id: f.clip,
            fades: Default::default(),
        }),
        case!("SetClipPlaybackDirection", false, |f| {
            C::SetClipPlaybackDirection {
                track_id: f.audio,
                clip_id: f.clip,
                direction: Default::default(),
            }
        }),
        case!(
            "SetClipWarpMarkers",
            false,
            |commands, f| {
                commands
                    .push(C::SetClipWarpMarkers {
                        track_id: f.audio,
                        clip_id: f.clip,
                        warp_markers: nonempty_warp_markers(),
                    })
                    .unwrap();
            },
            |f| C::SetClipWarpMarkers {
                track_id: f.audio,
                clip_id: f.clip,
                warp_markers: nonempty_warp_markers(),
            }
        ),
        case!("SetClipWarpMarkersMissingTrack", false, |f| {
            C::SetClipWarpMarkers {
                track_id: TrackId::new(),
                clip_id: f.clip,
                warp_markers: nonempty_warp_markers(),
            }
        }),
        case!("SetClipWarpMarkersMissingClip", false, |f| {
            C::SetClipWarpMarkers {
                track_id: f.empty_audio,
                clip_id: f.clip,
                warp_markers: nonempty_warp_markers(),
            }
        }),
        case!("SetClipBounds", false, |f| C::SetClipBounds {
            track_id: f.audio,
            clip_id: f.clip,
            source_offset: 0,
            start_marker: 0,
            duration: 2048,
            loop_start: 0,
            loop_end: 0,
        }),
        case!("SetTrackGain", false, |f| C::SetTrackGain(f.synth, 0.5)),
        case!("SetAutomationLane", false, |f| C::SetAutomationLane {
            track_id: f.empty,
            insertion: crate::clip_insertion::PreparedAutomationLaneInsertion::new(AutomationLane::new(AutomationTarget::TrackPan)),
        }),
        case!("RemoveAutomationLane", false, |f| C::RemoveAutomationLane {
            track_id: f.synth,
            lane_id: f.lane,
        }),
        case!("SetTrackPan", false, |f| C::SetTrackPan(f.synth, -0.5)),
        case!("SetTrackMute", false, |f| C::SetTrackMute(f.synth, true)),
        case!("QueueTrackMute", false, |f| C::QueueTrackMute {
            track_id: f.synth,
            muted: true,
            quantization: SectionLaunchQuantization::OneBar,
        }),
        case!("SetAutomationOverride", false, |f| {
            C::SetAutomationOverride {
                track_id: f.synth,
                target: AutomationTarget::TrackPan,
                overridden: true,
            }
        }),
        case!("UpdateAutomationGesture", false, |f| {
            C::UpdateAutomationGesture {
                track_id: f.synth,
                target: AutomationTarget::TrackPan,
                normalized_value: 0.25,
                begin: true,
            }
        }),
        case!("EndAutomationGesture", false, |f| C::EndAutomationGesture {
            track_id: f.synth,
            target: AutomationTarget::TrackPan,
        }),
        case!("SetTrackSolo", false, |f| C::SetTrackSolo(f.synth, true)),
        case!("SetTrackSwingOffset", false, |f| C::SetTrackSwingOffset(
            f.synth, None
        )),
        case!("AddBus", false, |_| C::add_bus(TrackId::new())),
        case!("RemoveBus", false, |f| C::RemoveBus(f.bus)),
        case!("SetSend", false, |f| C::SetSend {
            track_id: f.empty,
            bus_id: f.bus,
            amount: 0.5,
        }),
        case!("SetSampleRate", false, |_| C::SetSampleRate(48_000)),
        case!("SetSpectrumTap", false, |f| C::SetSpectrumTap(Some(
            f.synth
        ))),
        case!("RemoveEffect", false, |f| C::RemoveEffect(
            f.synth, f.effect
        )),
        case!("SetEffectParam", false, |f| C::SetEffectParam {
            track_id: f.synth,
            effect_id: f.effect,
            param_index: 0,
            value: 0.5,
        }),
        case!("SetEffectBypass", false, |f| C::SetEffectBypass {
            track_id: f.synth,
            effect_id: f.effect,
            bypass: true,
        }),
        case!("MoveEffect", false, |f| C::MoveEffect {
            track_id: f.synth,
            effect_id: f.effect,
            new_index: 0,
        }),
        case!(
            "RemoveTrackInstrument",
            false,
            |f| C::RemoveTrackInstrument(f.synth)
        ),
        case!("SetNoteClipDuration", false, |f| C::SetNoteClipDuration {
            track_id: f.synth,
            clip_id: f.note_clip,
            duration_beats: 2.0,
        }),
        case!("SetNoteClipGrooveGrid", false, |f| {
            C::SetNoteClipGrooveGrid {
                track_id: f.synth,
                clip_id: f.note_clip,
                groove_grid: Default::default(),
            }
        }),
        case!("AddNoteClip", false, |f| add_note_clip(
            f.empty,
            ClipId::new()
        )),
        case!("RemoveNoteClip", false, |f| C::RemoveNoteClip(
            f.synth,
            f.note_clip
        )),
        case!("MoveNoteClip", false, |f| C::MoveNoteClip {
            track_id: f.synth,
            clip_id: f.note_clip,
            new_position_beats: 1.0,
        }),
        case!("AddNote", false, |f| C::AddNote {
            track_id: f.synth,
            clip_id: f.empty_note_clip,
            insertion: crate::clip_insertion::PreparedNoteInsertion::new(note()),
        }),
        case!("RemoveNote", false, |f| C::RemoveNote {
            track_id: f.synth,
            clip_id: f.note_clip,
            note_index: 0,
        }),
        case!("EditNote", false, |f| C::EditNote {
            track_id: f.synth,
            clip_id: f.note_clip,
            note_index: 0,
            note: note(),
        }),
        case!("SetInstrumentParam", false, |f| C::SetInstrumentParam {
            track_id: f.synth,
            param_index: 0,
            value: 0.5,
        }),
        case!(
            "LoadSamplerSample",
            false,
            |commands, f| {
                prepare_populated_sampler_replacement(commands, f);
            },
            |f| C::LoadSamplerSample {
                track_id: f.empty,
                sample: audio(1024),
                sample_name: "Hit".into(),
            }
        ),
        case!("LoadDrumRackPadSample", false, |f| {
            C::LoadDrumRackPadSample {
                track_id: f.drums,
                pad_index: 0,
                sample: audio(1024),
                sample_name: "Kick".into(),
            }
        }),
        case!("ClearDrumRackPad", false, |f| C::ClearDrumRackPad {
            track_id: f.drums,
            pad_index: 0,
        }),
        case!(
            "SetDrumRackPadState",
            false,
            |f| C::set_drum_rack_pad_state(f.drums, 0, Default::default())
        ),
        case!("SetArrangementRecording", false, |_| {
            C::SetArrangementRecording(true)
        }),
        case!("SetArrangementLoop", false, |_| C::SetArrangementLoop(true)),
        case!("SetArrangementLoopRegion", false, |_| {
            C::SetArrangementLoopRegion {
                start: 0,
                end: 48_000,
            }
        }),
        case!("SetClipLoop", false, |f| C::SetClipLoop {
            track_id: f.audio,
            clip_id: f.clip,
            enabled: true,
            loop_start: 0,
            loop_end: 1024,
        }),
        case!("SetClipStartMarker", false, |f| C::SetClipStartMarker {
            track_id: f.audio,
            clip_id: f.clip,
            start_marker: 64,
        }),
        case!("SetNoteClipLoop", false, |f| C::SetNoteClipLoop {
            track_id: f.synth,
            clip_id: f.note_clip,
            enabled: true,
            loop_start_beats: 0.0,
            loop_end_beats: 2.0,
        }),
        case!("SetNoteClipStartMarker", false, |f| {
            C::SetNoteClipStartMarker {
                track_id: f.synth,
                clip_id: f.note_clip,
                start_marker_beats: 0.5,
            }
        }),
        case!("StartAudition", false, |_| C::StartAudition {
            audio: audio(4096),
            start: crate::commands::AuditionStart::Immediate,
            looped: false,
        }),
        case!("StopAudition", false, |_| C::StopAudition),
        case!("SetAuditionGain", false, |_| C::SetAuditionGain(0.5)),
        case!("ExternalNoteOn", false, |f| C::ExternalNoteOn {
            track_id: f.synth,
            pitch: 60,
            velocity: 100,
        }),
        case!("ExternalNoteOff", false, |f| C::ExternalNoteOff {
            track_id: f.synth,
            pitch: 60,
        }),
        case!("StartNoteRepeat", false, |f| C::StartNoteRepeat {
            id: 0,
            track_id: f.synth,
            pitch: 60,
            velocity: 100,
            rate: NoteRepeatRate::Eighth,
        }),
        case!(
            "UpdateNoteRepeatRate",
            false,
            |commands, f| {
                assert!(commands
                    .push(EngineCommand::StartNoteRepeat {
                        id: 0,
                        track_id: f.synth,
                        pitch: 60,
                        velocity: 100,
                        rate: NoteRepeatRate::Eighth,
                    })
                    .is_ok());
            },
            |f| C::UpdateNoteRepeatRate {
                id: 0,
                track_id: f.synth,
                rate: NoteRepeatRate::Quarter,
            }
        ),
        case!("StopNoteRepeat", false, |f| C::StopNoteRepeat {
            id: 0,
            track_id: f.synth,
        }),
        case!("InsertEffect", false, |f| C::InsertEffect {
            track_id: f.empty,
            effect_id: EffectId::new(),
            effect: vibez_dsp::factory::create_effect(
                vibez_core::effect::EffectType::Gain,
                48_000.0
            ),
            position: None,
        }),
        case!("AuditionNote", false, |f| C::AuditionNote {
            track_id: f.synth,
            pitch: 60,
            velocity: 100,
            on: true,
        }),
        case!("SetInstrument", false, |f| C::SetInstrument {
            track_id: f.empty,
            instrument: vibez_instruments::create_instrument(
                InstrumentKind::SubtractiveSynth,
                48_000.0
            ),
        }),
    ]
}

#[test]
fn populated_sampler_ratchet_fixture_proves_reset_owner_return_and_allocation_free_drain() {
    let (mut engine, mut commands, mut events, fixture) = playing(no_setup);
    let owner = prepare_populated_sampler_replacement(&mut commands, &fixture);
    for track in [fixture.synth, fixture.audio] {
        commands
            .push(EngineCommand::SetTrackMute(track, true))
            .unwrap();
    }
    // The existing mute ramp lasts 64 frames and sampler attack lasts 48 frames at 48kHz.
    engine.process(&mut [0.0; 128], 2);
    let mut output = [0.0; 32];
    engine.process(&mut output, 2);
    let expected = 0.125 * std::f32::consts::FRAC_1_SQRT_2;
    assert!(
        output
            .iter()
            .all(|sample| (*sample - expected).abs() < 1e-6),
        "settled original sampler expected={expected}, actual={output:?}"
    );
    while events.pop().is_ok() {}
    let instrument = engine
        .tracks()
        .iter()
        .find(|track| track.id == fixture.empty)
        .unwrap()
        .instrument
        .as_ref()
        .unwrap();
    assert_eq!(instrument.instrument_kind(), InstrumentKind::Sampler);
    assert_eq!(instrument.sample_name(), Some("Populated original sample"));
    assert_eq!(instrument.get_param(9), 0.5);
    let old_name_pointer = instrument.sample_name().unwrap().as_ptr();
    assert_eq!(
        owner.strong_count(),
        1,
        "installed sample is uniquely owned"
    );
    let case = cases()
        .into_iter()
        .find(|case| case.name == "LoadSamplerSample")
        .unwrap();
    let command = (case.command)(&fixture);
    assert!(
        matches!(command, EngineCommand::LoadSamplerSample { track_id, .. } if track_id == fixture.empty)
    );
    commands.push(command).unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut output, 2)),
        (0, 0)
    );
    assert_eq!(
        output, [0.0; 32],
        "replacement resets sounding sampler voices"
    );
    let instrument = engine
        .tracks()
        .iter()
        .find(|track| track.id == fixture.empty)
        .unwrap()
        .instrument
        .as_ref()
        .unwrap();
    assert_eq!(instrument.sample_name(), Some("Hit"));
    assert_eq!(instrument.get_param(9), 0.5);
    let mut retired = None;
    while let Ok(event) = events.pop() {
        if let EngineEvent::RetiredInstrumentMedia(owners) = event {
            assert!(retired.is_none());
            retired = Some(owners);
        }
    }
    let retired = retired.expect("exact complete displaced owners return from the table command");
    assert_eq!(
        retired.sample.as_ref().map(Arc::as_ptr),
        Some(owner.as_ptr())
    );
    assert_eq!(retired.name.as_ref().unwrap().as_ptr(), old_name_pointer);
    assert_eq!(owner.strong_count(), 1);
    drop(retired);
    assert_eq!(owner.strong_count(), 0);
    commands
        .push(EngineCommand::ExternalNoteOn {
            track_id: fixture.empty,
            pitch: 60,
            velocity: 127,
        })
        .unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut output, 2)),
        (0, 0)
    );
    for (frame, channels) in output.as_chunks::<2>().0.iter().enumerate() {
        let attack_level = expected * (frame + 1) as f32 / 48.0;
        assert!(
            channels
                .iter()
                .all(|sample| (*sample - attack_level).abs() < 1e-6),
            "new sampler attack frame={frame}, expected={attack_level}, actual={channels:?}"
        );
    }
    let mut attack_tail = [0.0; 128];
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut attack_tail, 2)),
        (0, 0)
    );
    assert!(
        attack_tail[96..]
            .iter()
            .all(|sample| (*sample - expected).abs() < 1e-6),
        "settled replacement sampler expected={expected}, actual={attack_tail:?}"
    );
}

#[test]
fn public_add_clip_at_exhausted_capacity_preserves_content_without_callback_heap_work() {
    use vibez_core::routing::RoutingChannel;
    use vibez_core::track::{ClipFades, ClipPlaybackDirection};
    use vibez_core::warp_marker::WarpMarkers;

    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track_id = TrackId::new();
    let resident_id = ClipId::new();
    let mut resident = add_clip(track_id, resident_id);
    let EngineCommand::AddClip {
        audio: resident_audio,
        duration,
        linear_gain,
        ..
    } = &mut resident
    else {
        unreachable!();
    };
    let resident_owner = Arc::downgrade(resident_audio);
    assert_eq!(resident_owner.strong_count(), 1);
    *duration = 512;
    *linear_gain = 0.5;
    commands.push(EngineCommand::add_track(track_id)).unwrap();
    commands.push(resident).unwrap();
    commands
        .push(EngineCommand::SetTrackPan(track_id, 0.0))
        .unwrap();
    let channels: Vec<_> = [track_id, TrackId::MASTER]
        .into_iter()
        .map(|id| RoutingChannel {
            id,
            is_bus: id.is_master(),
            sends: Vec::new(),
            effects: Vec::new(),
        })
        .collect();
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare(&channels, 8).unwrap(),
        ))
        .unwrap();
    engine.process_block(AudioProcessBlock::new(&mut [], 2));
    while events.pop().is_ok() {}

    let capacity = engine.tracks()[0].playback_source.clips.capacity();
    assert!(capacity > 0);
    // Vec growth is not part of the contract. Fill the actual allocation so
    // the measured insertion must cross its boundary on every allocator.
    while engine.tracks()[0].playback_source.clips.len() < capacity {
        let index = engine.tracks()[0].playback_source.clips.len();
        let mut filler = add_clip(track_id, ClipId::new());
        let EngineCommand::AddClip {
            position, duration, ..
        } = &mut filler
        else {
            unreachable!();
        };
        *position = 1024 + index as u64 * 64;
        *duration = 32;
        commands.push(filler).unwrap();
        engine.process_block(AudioProcessBlock::new(&mut [], 2));
        while events.pop().is_ok() {}
        assert_eq!(
            engine.tracks()[0].playback_source.clips.capacity(),
            capacity
        );
    }
    assert_eq!(engine.tracks()[0].playback_source.clips.len(), capacity);
    let snapshot = |clip: &crate::playback_source::EngineClip| {
        (
            clip.id,
            Arc::as_ptr(&clip.audio),
            [
                clip.position,
                clip.source_offset,
                clip.start_marker,
                clip.duration,
                clip.loop_start,
                clip.loop_end,
            ],
            clip.loop_enabled,
            clip.linear_gain,
            clip.fades,
            clip.playback_direction,
            clip.warp_markers.as_slice().as_ptr(),
            clip.warp_markers.clone(),
        )
    };
    let survivors: Vec<_> = engine.tracks()[0]
        .playback_source
        .clips
        .iter()
        .map(snapshot)
        .collect();
    let old_length = survivors
        .iter()
        .map(|clip| clip.2[0] + clip.2[3])
        .max()
        .unwrap();
    assert_eq!(engine.transport().audio_length(), Some(old_length));
    commands.push(EngineCommand::Play).unwrap();
    let mut output = [0.0; 16];
    engine.process_block(AudioProcessBlock::new(&mut output, 2));
    for &[left, right] in output.as_chunks::<2>().0 {
        assert!((left - 0.125).abs() < 1e-7, "pre-insertion left: {left}");
        assert_eq!(right, 0.0);
    }
    assert_eq!(engine.transport().position(), 8);
    assert_eq!(engine.transport().audio_length(), Some(old_length));
    while events.pop().is_ok() {}

    let incoming_id = ClipId::new();
    let incoming_audio = Arc::new(DecodedAudio {
        channels: vec![(0..256).map(|frame| frame as f32 / 256.0).collect()],
        sample_rate: DEFAULT_SAMPLE_RATE,
    });
    let incoming_owner = Arc::downgrade(&incoming_audio);
    assert_eq!(incoming_owner.strong_count(), 1);
    let mut markers = WarpMarkers::default();
    assert!(markers.add(39, 64, 7, 135, 128));
    let marker_allocation = markers.as_slice().as_ptr();
    let expected_markers = markers.clone();
    let duration = old_length + 128;
    let fades = ClipFades::new(2, 3, duration);
    commands
        .push(EngineCommand::AddClip {
            insertion: crate::clip_insertion::PreparedClipInsertion::new(),
            track_id,
            clip_id: incoming_id,
            audio: incoming_audio,
            position: 4,
            source_offset: 7,
            start_marker: 15,
            duration,
            loop_enabled: true,
            loop_start: 11,
            loop_end: 43,
            linear_gain: 0.5,
            fades,
            playback_direction: ClipPlaybackDirection::Forward,
            warp_markers: markers,
        })
        .unwrap();

    let exchange = fulfill_clip_insertions(&mut engine, &mut events);
    let render = crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(&mut output, 2));
    });
    let measured = (exchange.0 + render.0, exchange.1 + render.1);

    let clips = &engine.tracks()[0].playback_source.clips;
    assert_eq!(clips.len(), capacity + 1);
    assert_eq!(
        clips[..capacity].iter().map(snapshot).collect::<Vec<_>>(),
        survivors
    );
    assert_eq!(resident_owner.strong_count(), 1);
    assert_eq!(Arc::as_ptr(&clips[0].audio), resident_owner.as_ptr());
    let incoming = &clips[capacity];
    assert_eq!(incoming.id, incoming_id);
    assert_eq!(Arc::as_ptr(&incoming.audio), incoming_owner.as_ptr());
    assert_eq!(incoming_owner.strong_count(), 1);
    assert_eq!(incoming.position, 4);
    assert_eq!(incoming.source_offset, 7);
    assert_eq!(incoming.start_marker, 15);
    assert_eq!(incoming.duration, duration);
    assert!(incoming.loop_enabled);
    assert_eq!((incoming.loop_start, incoming.loop_end), (11, 43));
    assert_eq!(incoming.linear_gain, 0.5);
    assert_eq!(incoming.fades, fades);
    assert_eq!(incoming.playback_direction, ClipPlaybackDirection::Forward);
    assert_eq!(incoming.warp_markers.as_slice().as_ptr(), marker_allocation);
    assert_eq!(incoming.warp_markers, expected_markers);
    for (frame, &[left, right]) in output.as_chunks::<2>().0.iter().enumerate() {
        // At cursor 8 the Clip-local frame is 4. The first Warp segment maps
        // source 7 + (start 15 + local 4 - offset 7) / 2 to source 13.
        let expected = 0.125 + (13.0 + frame as f32 * 0.5) / 256.0 * 0.5;
        assert!(
            (left - expected).abs() < 1e-7,
            "post-insertion frame {frame}: expected {expected}, actual {left}"
        );
        assert_eq!(right, 0.0);
    }
    assert_eq!(engine.transport().position(), 16);
    assert!(engine.transport().is_playing());
    assert_eq!(engine.transport().audio_length(), Some(4 + duration));
    assert_eq!(measured, (0, 0), "exhausted-capacity AddClip: {measured:?}");
}

#[test]
fn public_add_clip_to_missing_track_returns_unique_payload_without_callback_heap_work() {
    use vibez_core::routing::RoutingChannel;
    use vibez_core::track::{ClipFades, ClipPlaybackDirection};
    use vibez_core::warp_marker::WarpMarkers;

    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track_id = TrackId::new();
    let resident_id = ClipId::new();
    let resident = add_clip(track_id, resident_id);
    let EngineCommand::AddClip {
        audio: resident_audio,
        ..
    } = &resident
    else {
        unreachable!();
    };
    let resident_owner = Arc::downgrade(resident_audio);
    commands.push(EngineCommand::add_track(track_id)).unwrap();
    commands.push(resident).unwrap();
    commands
        .push(EngineCommand::SetTrackPan(track_id, 0.0))
        .unwrap();
    let channels: Vec<_> = [track_id, TrackId::MASTER]
        .into_iter()
        .map(|id| RoutingChannel {
            id,
            is_bus: id.is_master(),
            sends: Vec::new(),
            effects: Vec::new(),
        })
        .collect();
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare(&channels, 8).unwrap(),
        ))
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut output = [0.0; 16];
    engine.process_block(AudioProcessBlock::new(&mut output, 2));
    for &[left, right] in output.as_chunks::<2>().0 {
        assert!((left - 0.25).abs() < 1e-7);
        assert_eq!(right, 0.0);
    }
    assert_eq!(engine.transport().position(), 8);
    assert_eq!(engine.transport().audio_length(), Some(4096));
    while events.pop().is_ok() {}

    let missing_track = TrackId::new();
    assert!(engine
        .tracks()
        .iter()
        .all(|track| track.id != missing_track));
    let incoming_audio = audio(256);
    let incoming_owner = Arc::downgrade(&incoming_audio);
    assert_eq!(incoming_owner.strong_count(), 1);
    let mut markers = WarpMarkers::default();
    assert!(markers.add(39, 64, 7, 135, 128));
    assert_eq!(markers.as_slice().len(), 3);
    commands
        .push(EngineCommand::AddClip {
            insertion: crate::clip_insertion::PreparedClipInsertion::new(),
            track_id: missing_track,
            clip_id: ClipId::new(),
            audio: incoming_audio,
            position: 8192,
            source_offset: 7,
            start_marker: 15,
            duration: 128,
            loop_enabled: true,
            loop_start: 11,
            loop_end: 43,
            linear_gain: 0.5,
            fades: ClipFades::new(2, 3, 128),
            playback_direction: ClipPlaybackDirection::Reverse,
            warp_markers: markers,
        })
        .unwrap();

    let measured = crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(&mut output, 2));
    });
    let retained_after_callback = incoming_owner.strong_count();
    let mut returned_events = Vec::new();
    while let Ok(event) = events.pop() {
        returned_events.push(event);
    }
    assert_eq!(engine.tracks().len(), 1);
    let clips = &engine.tracks()[0].playback_source.clips;
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].id, resident_id);
    assert_eq!(Arc::as_ptr(&clips[0].audio), resident_owner.as_ptr());
    assert_eq!(resident_owner.strong_count(), 1);
    assert_eq!(engine.transport().audio_length(), Some(4096));
    assert_eq!(engine.transport().position(), 16);
    assert!(engine.transport().is_playing());
    for &[left, right] in output.as_chunks::<2>().0 {
        assert!((left - 0.25).abs() < 1e-7);
        assert_eq!(right, 0.0);
    }
    drop(returned_events);
    assert_eq!(incoming_owner.strong_count(), 0);
    assert_eq!(
        (measured, retained_after_callback),
        ((0, 0), 1),
        "missing-track AddClip must retain the complete payload for UI destruction"
    );
}

#[test]
fn every_table_command_drains_without_allocating_unless_named_as_card_04_work() {
    let mut mismatches = Vec::new();
    for case in cases() {
        let (mut engine, mut commands, _events, fixture) = playing(case.setup);
        let command = (case.command)(&fixture);
        if let Coverage::Elsewhere(test) = coverage(&command) {
            panic!("{} is measured by {test}", case.name);
        }
        let added_note = match &command {
            EngineCommand::AddNote {
                track_id, clip_id, ..
            } => Some((*track_id, *clip_id, note())),
            _ => None,
        };
        assert!(commands.push(command).is_ok());
        let mut output = [0.0_f32; 512];
        let measured = crate::retirement::tests::allocations(|| engine.process(&mut output, 2));
        if let Some((track_id, clip_id, note)) = added_note {
            let track = engine
                .tracks()
                .iter()
                .find(|track| track.id == track_id)
                .unwrap();
            let clip = track
                .playback_source
                .note_clips
                .iter()
                .find(|clip| clip.id == clip_id)
                .unwrap();
            assert_eq!(clip.notes, [note]);
        }
        if (measured != (0, 0)) != case.allocates {
            mismatches.push(format!(
                "{}: measured {measured:?}, declared allocates={}",
                case.name, case.allocates
            ));
        }
    }
    assert!(mismatches.is_empty(), "{mismatches:#?}");
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "the UI keeps device chains within DEVICE_CHAIN_CAPACITY")]
fn a_device_past_the_reserved_chain_is_a_ui_contract_violation() {
    let (mut engine, mut commands, _events) = AudioEngine::new();
    let track_id = TrackId::new();
    assert!(commands.push(EngineCommand::add_track(track_id)).is_ok());
    engine.process(&mut [0.0; 2], 2);
    for _ in 0..=crate::mixer::DEVICE_CHAIN_CAPACITY {
        assert!(commands
            .push(EngineCommand::insert_built_in_effect(
                track_id,
                EffectId::new(),
                vibez_core::effect::EffectType::Gain,
                crate::engine::DEFAULT_SAMPLE_RATE,
            ))
            .is_ok());
        engine.process(&mut [0.0; 2], 2);
    }
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "the UI keeps channels within CHANNEL_CAPACITY")]
fn a_channel_past_the_reserved_list_is_a_ui_contract_violation() {
    let (mut engine, mut commands, _events) = AudioEngine::new();
    for _ in 0..=crate::mixer::CHANNEL_CAPACITY {
        assert!(commands
            .push(EngineCommand::add_track(TrackId::new()))
            .is_ok());
        engine.process(&mut [0.0; 2], 2);
    }
}
