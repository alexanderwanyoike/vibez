//! Public automation insertion preserves live sound and resident owners at the outer storage boundary.

use super::*;
use crate::mixer::EngineNoteClip;
use crate::playback_source::PreparedPlaybackSource;
use crate::test_support::DelayProbeInstrument;
use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
use vibez_core::id::ClipId;
use vibez_core::midi::MidiNote;
use vibez_core::perform::GrooveGrid;
use vibez_core::routing::RoutingChannel;

#[test]
fn appending_a_lane_at_full_capacity_preserves_audio_and_owners_without_callback_allocation() {
    let track_id = TrackId::new();
    let voice_id = TrackId::new();
    let audio_id = ClipId::new();
    let note_id = ClipId::new();
    let audio = Arc::new(DecodedAudio {
        channels: vec![vec![0.25; 4096]; 2],
        sample_rate: 48_000,
    });
    let audio_owner = Arc::downgrade(&audio);
    let mut pan = AutomationLane::new(AutomationTarget::TrackPan);
    pan.points = vec![
        AutomationPoint { beat: 0.0, value: 0.0, curve: 0.5 },
        AutomationPoint { beat: 4.0, value: 0.0, curve: -0.5 },
    ];
    let mut mute = AutomationLane::new(AutomationTarget::TrackMute);
    mute.points.push(AutomationPoint { beat: 0.0, value: 0.0, curve: 0.0 });
    let mut gain = AutomationLane::new(AutomationTarget::TrackGain);
    gain.points = vec![
        AutomationPoint { beat: 0.0, value: 0.25, curve: -0.5 },
        AutomationPoint { beat: 4.0, value: 0.25, curve: 0.75 },
    ];
    let expected_lanes = [pan.clone(), mute.clone(), gain.clone()];
    let point_owners = [pan.points.as_ptr(), mute.points.as_ptr(), gain.points.as_ptr()];
    let lanes = vec![pan, mute].into_boxed_slice().into_vec();
    assert_eq!((lanes.len(), lanes.capacity()), (2, 2));
    let source = PreparedPlaybackSource::new(
        vec![EngineClip {
            id: audio_id,
            audio,
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
        }],
        vec![],
        lanes,
    );
    let target = Box::new(EngineTrack::with_playback_source(track_id, source));
    let source_owner = std::ptr::from_ref(target.playback_source.as_ref());
    let clips_owner = target.playback_source.clips.as_ptr();
    let notes = vec![
        MidiNote { pitch: 60, velocity: 127, start_beat: 0.0, duration_beats: 4.0 },
        MidiNote { pitch: 60, velocity: 100, start_beat: 2.0, duration_beats: 1.0 },
    ];
    let expected_notes = notes.clone();
    let note = EngineNoteClip::new(
        note_id, 0.0, 4.0, notes, 0.0, false, 0.0, 0.0, GrooveGrid::Eighth,
    );
    let mut voice = Box::new(EngineTrack::with_playback_source(
        voice_id,
        PreparedPlaybackSource::new(vec![], vec![note], vec![]),
    ));
    voice.instrument = Some(Box::new(DelayProbeInstrument::new(0, 0.125)));
    let voice_source_owner = std::ptr::from_ref(voice.playback_source.as_ref());
    let note_clips_owner = voice.playback_source.note_clips.as_ptr();
    let notes_owner = voice.playback_source.note_clips[0].notes.as_ptr();
    let instrument_owner = std::ptr::from_ref(voice.instrument.as_deref().unwrap());
    let swing = SwingAmount::new(0.56);

    let channels = [track_id, voice_id, TrackId::MASTER].map(|id| RoutingChannel {
        id,
        is_bus: id.is_master(),
        sends: vec![],
        effects: vec![],
    });
    let mut routing = crate::routing::PreparedRouting::prepare(&channels, 64).unwrap();
    routing.configure_automation(&[
        (track_id, AutomationTarget::TrackPan),
        (track_id, AutomationTarget::TrackMute),
        (track_id, AutomationTarget::TrackGain),
    ]).unwrap();
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    for command in [
        EngineCommand::SetSampleRate(48_000),
        EngineCommand::SetBpm(120.0),
        EngineCommand::SetProjectSwing(swing),
        EngineCommand::AddTrack(target),
        EngineCommand::AddTrack(voice),
        EngineCommand::SetTrackGain(track_id, 1.0),
        EngineCommand::SetTrackGain(voice_id, 1.0),
        EngineCommand::SetTrackGain(TrackId::MASTER, 1.0),
        EngineCommand::SetTrackPan(voice_id, 1.0),
        EngineCommand::SetRouting(routing),
        EngineCommand::Play,
    ] {
        commands.push(command).unwrap();
    }
    let mut output = [0.0; 128];
    let mut capture = [0.0; 128];
    for _ in 0..4 {
        engine.process_block(AudioProcessBlock::new(&mut output, 2)
            .with_track_output_capture(track_id.raw(), &mut capture));
        while events.pop().is_ok() {}
    }
    assert!(engine.cmd_rx.peek().is_err());
    assert_eq!(engine.transport().position(), 256);
    assert!(engine.transport().is_playing());
    assert_eq!(engine.transport().audio_length(), Some(96_000));
    for frame in capture.as_chunks::<2>().0 {
        assert_eq!(*frame, [0.25, 0.0]);
    }
    for frame in output.as_chunks::<2>().0 {
        assert_eq!(frame[1], 0.125);
        assert!((frame[0] - 0.25).abs() < f32::EPSILON);
    }
    let assert_content = |engine: &AudioEngine, expected: &[AutomationLane]| {
        let target = engine.tracks().iter().find(|track| track.id == track_id).unwrap();
        assert!(std::ptr::eq(target.playback_source.as_ref(), source_owner));
        assert_eq!(target.playback_source.clips.as_ptr(), clips_owner);
        assert_eq!(target.playback_source.clips.len(), 1);
        let clip = &target.playback_source.clips[0];
        assert_eq!((clip.id, clip.position, clip.source_offset, clip.start_marker, clip.duration),
            (audio_id, 0, 0, 0, 4096));
        assert_eq!((clip.loop_enabled, clip.loop_start, clip.loop_end, clip.linear_gain),
            (false, 0, 0, 1.0));
        assert_eq!(audio_owner.strong_count(), 1);
        let lanes = &target.playback_source.automation;
        assert_eq!(lanes, expected);
        for (lane, owner) in lanes.iter().zip(point_owners) {
            assert_eq!(lane.points.as_ptr(), owner);
        }
        let voice = engine.tracks().iter().find(|track| track.id == voice_id).unwrap();
        assert!(std::ptr::eq(voice.playback_source.as_ref(), voice_source_owner));
        assert_eq!(voice.playback_source.note_clips.as_ptr(), note_clips_owner);
        assert_eq!(voice.playback_source.note_clips.len(), 1);
        let clip = &voice.playback_source.note_clips[0];
        assert_eq!((clip.id, clip.position_beats, clip.duration_beats, clip.groove_grid),
            (note_id, 0.0, 4.0, GrooveGrid::Eighth));
        assert_eq!((clip.start_marker_beats, clip.loop_enabled, clip.loop_start_beats, clip.loop_end_beats),
            (0.0, false, 0.0, 0.0));
        assert_eq!(clip.notes, expected_notes);
        assert_eq!(clip.notes.as_ptr(), notes_owner);
        assert_eq!(clip.next_same_pitch_start_beat(0), Some(2.0));
        assert_eq!(clip.next_same_pitch_start_beat(1), None);
        // A different current Swing must not replace this already sounding pair's latch.
        assert_eq!(clip.swing_for_pair(0.0, SwingAmount::new(0.70), false), swing);
        assert!(std::ptr::eq(std::ptr::from_ref(voice.instrument.as_deref().unwrap()), instrument_owner));
    };
    assert_content(&engine, &expected_lanes[..2]);
    let resident = &engine.tracks()[0].playback_source.automation;
    assert_eq!((resident.len(), resident.capacity()), (2, 2));
    let before = engine.transport().position();
    let length = engine.transport().audio_length();
    commands.push(EngineCommand::SetAutomationLane { track_id, lane: gain }).unwrap();

    let measured = crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(&mut output, 2)
            .with_track_output_capture(track_id.raw(), &mut capture));
    });
    assert!(engine.cmd_rx.peek().is_err());
    assert_eq!(engine.transport().position(), before + 64);
    assert_eq!(engine.transport().audio_length(), length);
    assert!(engine.transport().is_playing());
    assert_content(&engine, &expected_lanes);
    for frame in capture.as_chunks::<2>().0 {
        assert_eq!(*frame, [0.125, 0.0]);
    }
    for frame in output.as_chunks::<2>().0 {
        assert_eq!(frame[1], 0.125);
        assert!((frame[0] - 0.125).abs() < f32::EPSILON);
    }
    eprintln!("automation-lane-insertion: before={before}; after={}; length={length:?}; lanes=3; old_point_owners=true; incoming_point_owner=true; source_owners=true; note_cache=true; sounding_voice=true; groove_latch=true; callback_heap={measured:?}; capture_bits={:?}; output_bits={:?}",
        engine.transport().position(), capture.map(f32::to_bits), output.map(f32::to_bits));
    assert_eq!(measured, (0, 0));
}
