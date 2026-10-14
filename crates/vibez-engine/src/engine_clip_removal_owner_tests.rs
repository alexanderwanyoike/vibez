//! Clip retirement retains complete owners across resident source homes and command backpressure.

use super::*;
use crate::playback_source::PreparedPlaybackSource;
use vibez_core::{id::ClipId, midi::MidiNote, perform::GrooveGrid};

fn note_clip(id: ClipId) -> EngineNoteClip {
    EngineNoteClip::new(id, 0.0, 4.0, vec![MidiNote {
        pitch: 60, velocity: 100, start_beat: 0.0, duration_beats: 3.0,
    }], 0.0, false, 0.0, 4.0, GrooveGrid::Off)
}

fn audio_clip(id: ClipId, value: Arc<DecodedAudio>) -> EngineClip {
    let mut markers = vibez_core::warp_marker::WarpMarkers::default();
    assert!(markers.add(32, 32, 0, 128, 128));
    EngineClip { id, audio: value, position: 0, source_offset: 0, start_marker: 0,
        duration: 128, loop_enabled: false, loop_start: 0, loop_end: 128,
        linear_gain: 1.0, fades: Default::default(), playback_direction: Default::default(),
        warp_markers: markers }
}

#[test]
fn every_source_home_returns_complete_staged_owners_before_channel_removal() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track_id = TrackId::new();
    let id = ClipId::new();
    let mut channel = EngineTrack::new(track_id);
    let mut audio_owners = Vec::new();
    let mut marker_pointers = Vec::new();
    let mut note_pointers = Vec::new();
    for home in [&mut channel.playback_source, &mut channel.section_playback_source,
        &mut channel.launcher_source, &mut channel.empty_launcher_source] {
        let mut clips = Vec::new();
        let mut notes = Vec::new();
        for _ in 0..9 {
            let audio = Arc::new(DecodedAudio { channels: vec![vec![0.25; 128]], sample_rate: 44_100 });
            audio_owners.push(Arc::downgrade(&audio));
            let clip = audio_clip(id, audio);
            marker_pointers.push(clip.warp_markers.as_slice().as_ptr());
            clips.push(clip);
            let note = note_clip(id);
            note_pointers.push(note.retirement_storage());
            notes.push(note);
        }
        **home = PreparedPlaybackSource::new(clips, notes, vec![]);
        home.remove_audio_clips(id);
        home.remove_note_clips(id);
    }
    commands.push(EngineCommand::AddTrack(Box::new(channel))).unwrap();
    commands.push(EngineCommand::RemoveTrack(track_id)).unwrap();
    while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
    assert_eq!(crate::retirement::tests::allocations(|| engine.process(&mut [], 2)), (0, 0));
    assert_eq!(engine.pending_retirements.len(), crate::retirement::RETIREMENT_CAPACITY);
    assert_eq!(engine.cmd_rx.slots(), 1);
    assert_eq!(engine.tracks().len(), 1);
    assert!(audio_owners.iter().all(|owner| owner.strong_count() == 1));
    let mut returned_audio = Vec::new();
    let mut returned_markers = Vec::new();
    let mut returned_notes = Vec::new();
    for _ in 0..4 {
        while let Ok(event) = events.pop() {
            assert_eq!(event.time_domain(), crate::events::TimeDomain::Untimed);
            match event {
                EngineEvent::RetiredAudioClip(clip) => {
                    returned_audio.push(Arc::as_ptr(&clip.audio));
                    returned_markers.push(clip.warp_markers.as_slice().as_ptr());
                }
                EngineEvent::RetiredNoteClip(clip) => returned_notes.push(clip.retirement_storage()),
                _ => {}
            }
        }
        assert_eq!(crate::retirement::tests::allocations(|| engine.process(&mut [], 2)), (0, 0));
    }
    assert!(engine.tracks().is_empty());
    assert_eq!(engine.cmd_rx.slots(), 0);
    assert_eq!(returned_audio.len(), 36);
    assert_eq!(returned_notes.len(), 36);
    for owner in &audio_owners {
        assert_eq!(returned_audio.iter().filter(|pointer| **pointer == owner.as_ptr()).count(), 1);
        assert_eq!(owner.strong_count(), 0);
    }
    for pointer in marker_pointers {
        assert_eq!(returned_markers.iter().filter(|returned| **returned == pointer).count(), 1);
    }
    for pointers in note_pointers {
        assert_eq!(returned_notes.iter().filter(|returned| **returned == pointers).count(), 1);
    }
}

#[test]
fn exactly_full_retirement_storage_keeps_removal_queued_until_ui_delivery() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let id = ClipId::new();
    let audio = Arc::new(DecodedAudio { channels: vec![vec![0.25; 128]], sample_rate: 44_100 });
    let owner = Arc::downgrade(&audio);
    let source = PreparedPlaybackSource::new(vec![audio_clip(id, audio)], vec![], vec![]);
    commands.push(EngineCommand::AddTrack(Box::new(EngineTrack::with_playback_source(track, source)))).unwrap();
    engine.process(&mut [], 2);
    while events.pop().is_ok() {}
    while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
    for _ in 0..crate::retirement::RETIREMENT_CAPACITY {
        commands.push(EngineCommand::ReorderTracks(vec![track])).unwrap();
    }
    commands.push(EngineCommand::RemoveClip(track, id)).unwrap();
    assert_eq!(crate::retirement::tests::allocations(|| engine.process(&mut [], 2)), (0, 0));
    assert_eq!(engine.pending_retirements.len(), crate::retirement::RETIREMENT_CAPACITY);
    assert_eq!(engine.cmd_rx.slots(), 1);
    assert_eq!(engine.tracks()[0].playback_source.clips.len(), 1);
    assert_eq!(owner.strong_count(), 1);
    while events.pop().is_ok() {}
    assert_eq!(crate::retirement::tests::allocations(|| engine.process(&mut [], 2)), (0, 0));
    assert_eq!(engine.cmd_rx.slots(), 0);
    assert!(engine.tracks()[0].playback_source.clips.is_empty());
    assert_eq!(owner.strong_count(), 1);
    while events.pop().is_ok() {}
    assert_eq!(owner.strong_count(), 0);
}

#[test]
fn empty_missing_and_repeated_removals_keep_note_flush_and_length_semantics() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let id = ClipId::new();
    commands.push(EngineCommand::add_track(track)).unwrap();
    commands.push(EngineCommand::SetInstrument { track_id: track,
        instrument: Box::new(crate::test_support::DelayProbeInstrument::new(0, 0.25)) }).unwrap();
    commands.push(EngineCommand::NoteOn(track, 60, 100)).unwrap();
    let mut output = [0.0; 16];
    engine.process(&mut output, 2);
    assert!(output.iter().all(|sample| *sample > 0.1));
    while events.pop().is_ok() {}
    commands.push(EngineCommand::RemoveNoteClip(TrackId::new(), id)).unwrap();
    assert_eq!(crate::retirement::tests::allocations(|| engine.process(&mut output, 2)), (0, 0));
    assert!(output.iter().all(|sample| *sample > 0.1));
    for command in [EngineCommand::RemoveClip(TrackId::new(), id),
        EngineCommand::RemoveClip(track, id), EngineCommand::RemoveNoteClip(track, id),
        EngineCommand::RemoveClip(track, id), EngineCommand::RemoveNoteClip(track, id)] {
        commands.push(command).unwrap();
    }
    assert_eq!(crate::retirement::tests::allocations(|| engine.process(&mut output, 2)), (0, 0));
    assert_eq!(output, [0.0; 16]);
    assert_eq!(engine.transport().audio_length(), None);
    assert!(std::iter::from_fn(|| events.pop().ok()).all(|event| !matches!(event,
        EngineEvent::RetiredAudioClip(_) | EngineEvent::RetiredNoteClip(_))));
}
