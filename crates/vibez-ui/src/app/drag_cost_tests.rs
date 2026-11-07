//! What each step of an Arrange clip drag costs the engine, measured through the real router.

use super::*;
use crate::domains::arrangement::ArrangementMsg;
use crate::state::{ArrangementSelection, ProjectTrack, UiClip, UiNoteClip, UndoGestureId};
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::{MidiNote, TrackKind};

const STEPS: usize = 10;
const LONG_CLIP_NOTES: usize = 512;

fn audio_clip(position: u64, duration: u64) -> UiClip {
    UiClip {
        id: ClipId::new(),
        name: "Loop".into(),
        audio: Arc::new(DecodedAudio {
            channels: vec![vec![0.25; (duration * 2) as usize]],
            sample_rate: 48_000,
        }),
        source: crate::state::UiClip::test_source(),
        position,
        source_offset: 0,
        start_marker: 0,
        duration,
        loop_enabled: false,
        loop_start: 0,
        loop_end: duration,
        gain_db: Default::default(),
        fades: Default::default(),
        playback_direction: Default::default(),
        transient_markers: Default::default(),
        warp_markers: Default::default(),
        transpose: Default::default(),
        original_bpm: Some(120.0),
        warped: false,
        warped_to_bpm: None,
        original_audio: None,
    }
}

fn note_clip(position_beats: f64, duration_beats: f64, notes: usize) -> UiNoteClip {
    UiNoteClip {
        name: "Pattern".into(),
        notes: (0..notes)
            .map(|step| MidiNote {
                pitch: 36 + (step % 24) as u8,
                velocity: 100,
                start_beat: step as f64 * duration_beats / notes as f64,
                duration_beats: 0.25,
            })
            .collect(),
        loop_enabled: true,
        ..UiNoteClip::test_clip(position_beats, duration_beats)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct Cost {
    commands: usize,
    insertions: usize,
    removals: usize,
}

impl Cost {
    fn of(sent: &[EngineCommand]) -> Self {
        sent.iter().fold(Self::default(), |mut cost, command| {
            cost.commands += 1;
            match command {
                EngineCommand::AddClip { .. }
                | EngineCommand::AddNoteClip { .. }
                | EngineCommand::AddNote { .. } => cost.insertions += 1,
                EngineCommand::RemoveClip(..) | EngineCommand::RemoveNoteClip(..) => {
                    cost.removals += 1
                }
                _ => {}
            }
            cost
        })
    }

    fn per_step(total: Self) -> Self {
        Self {
            commands: total.commands / STEPS,
            insertions: total.insertions / STEPS,
            removals: total.removals / STEPS,
        }
    }
}

struct Song {
    app: App,
    commands: rtrb::Consumer<EngineCommand>,
    audio: [TrackId; 2],
    midi: [TrackId; 2],
    loop_clip: ClipId,
    long_clip: ClipId,
}

/// Two audio and two MIDI lanes; every lane already holds clips so a
/// cross-lane drop lands beside resident content, as in a real song.
fn song() -> Song {
    let mut app = super::test_support::app();
    let (producer, commands) = rtrb::RingBuffer::new(1 << 16);
    app.cmd_tx = crate::domains::EngineCommandQueue::new(
        producer,
        vibez_engine::transport_ring::transport_ring().0,
    );
    let spb = app.state.transport.samples_per_beat().round() as u64;
    let mut lane = |kind: TrackKind, index: u8| {
        let mut track = ProjectTrack::new(TrackId::new(), format!("Lane {index}"), index);
        track.kind = kind;
        let id = track.id;
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(track);
        id
    };
    let audio = [lane(TrackKind::Audio, 0), lane(TrackKind::Audio, 1)];
    let midi = [lane(TrackKind::Midi, 2), lane(TrackKind::Midi, 3)];
    let timeline = Arc::make_mut(&mut app.state.arrangement.timeline);
    let loop_clip = audio_clip(0, 8 * spb);
    let loop_id = loop_clip.id;
    timeline.ensure(audio[0]).clips.push(loop_clip);
    for bar in 0..4 {
        timeline
            .ensure(audio[1])
            .clips
            .push(audio_clip(64 * spb + bar * 8 * spb, 4 * spb));
    }
    let long = note_clip(0.0, 64.0, LONG_CLIP_NOTES);
    let long_id = long.id;
    timeline.ensure(midi[0]).note_clips.push(long);
    for bar in 0..4 {
        timeline
            .ensure(midi[1])
            .note_clips
            .push(note_clip(128.0 + bar as f64 * 4.0, 4.0, 8));
    }
    Song {
        app,
        commands,
        audio,
        midi,
        loop_clip: loop_id,
        long_clip: long_id,
    }
}

impl Song {
    /// Presses on `grabbed` the way the lane canvas does, then drags it.
    fn drag(
        &mut self,
        grabbed: ArrangementSelection,
        steps: impl Fn(usize) -> ArrangementMsg,
    ) -> Cost {
        let _ = self.app.update_and_refresh_clips(Message::Arrangement(
            ArrangementMsg::SelectArrangementClip {
                selection: grabbed,
                shift_held: false,
            },
        ));
        while self.commands.pop().is_ok() {}
        let gesture = UndoGestureId::new();
        let mut total = Cost::default();
        for step in 1..=STEPS {
            let _ = self.app.update_and_refresh_clips(Message::UndoGesture {
                id: gesture,
                edit: Box::new(Message::Arrangement(steps(step))),
            });
            let sent: Vec<_> = std::iter::from_fn(|| self.commands.pop().ok()).collect();
            let cost = Cost::of(&sent);
            total.commands += cost.commands;
            total.insertions += cost.insertions;
            total.removals += cost.removals;
        }
        Cost::per_step(total)
    }
}

fn assert_in_place(costs: &[(&str, Cost)]) {
    let in_place = Cost {
        commands: 1,
        ..Cost::default()
    };
    assert!(
        costs.iter().all(|(_, cost)| *cost == in_place),
        "per drag step: {costs:#?}"
    );
}

#[test]
fn midi_move_and_resize_steps_are_each_one_in_place_engine_edit() {
    let mut song = song();
    let [midi, _] = song.midi;
    let long_clip = ArrangementSelection::NoteClip {
        track_id: midi,
        clip_id: song.long_clip,
    };
    assert_in_place(&[
        (
            "MIDI move",
            song.drag(long_clip, |step| ArrangementMsg::MoveSelectedClips {
                anchor: long_clip.clip_id(),
                new_position_beats: step as f64,
                target_track: midi,
            }),
        ),
        (
            "MIDI resize",
            song.drag(long_clip, |step| ArrangementMsg::ResizeSelectedClips {
                anchor: long_clip,
                new_duration_beats: 64.0 - step as f64,
            }),
        ),
    ]);
}

#[test]
fn audio_move_and_trim_steps_are_each_one_in_place_engine_edit() {
    let mut song = song();
    let [audio, _] = song.audio;
    let loop_clip = ArrangementSelection::AudioClip {
        track_id: audio,
        clip_id: song.loop_clip,
    };
    assert_in_place(&[
        (
            "audio move",
            song.drag(loop_clip, |step| ArrangementMsg::MoveSelectedClips {
                anchor: loop_clip.clip_id(),
                new_position_beats: step as f64,
                target_track: audio,
            }),
        ),
        (
            "audio trim",
            song.drag(loop_clip, |step| ArrangementMsg::ResizeSelectedClips {
                anchor: loop_clip,
                new_duration_beats: 8.0 - step as f64 * 0.5,
            }),
        ),
    ]);
}

struct ResidentSong {
    song: Song,
    engine: vibez_engine::engine::AudioEngine,
}

/// The song resident in a real engine, with every storage request served.
fn resident_song() -> ResidentSong {
    let mut song = song();
    let (mut engine, commands, events) = vibez_engine::engine::AudioEngine::new();
    song.app.cmd_tx =
        crate::domains::EngineCommandQueue::new(commands, engine.take_transport_sender().unwrap());
    song.app.event_rx = Some(events);
    let tracks = song.app.state.project_tracks.tracks.clone();
    for track in &tracks {
        song.app.replay_track_to_engine(track);
    }
    song.app.sync_sidechain_routing();
    for _ in 0..16 {
        engine.process_block(vibez_engine::engine::AudioProcessBlock::new(&mut [], 2));
        song.app.poll_engine_events();
    }
    ResidentSong { song, engine }
}

impl ResidentSong {
    fn source(&self, track_id: TrackId) -> &vibez_engine::playback_source::PreparedPlaybackSource {
        &self
            .engine
            .tracks()
            .iter()
            .find(|track| track.id == track_id)
            .unwrap()
            .playback_source
    }
}

#[test]
fn a_clip_dragged_into_a_full_lane_plays_there_in_the_next_callback() {
    let mut resident = resident_song();
    let [audio, audio_target] = resident.song.audio;
    let [midi, midi_target] = resident.song.midi;
    let target_audio = resident.source(audio_target);
    let target_notes = resident.source(midi_target);
    assert_eq!(target_audio.clips.len(), target_audio.clips.capacity());
    assert_eq!(
        target_notes.note_clips.len(),
        target_notes.note_clips.capacity()
    );
    let (loop_clip, long_clip) = (resident.song.loop_clip, resident.song.long_clip);
    for (grabbed, target_track) in [
        (
            ArrangementSelection::AudioClip {
                track_id: audio,
                clip_id: loop_clip,
            },
            audio_target,
        ),
        (
            ArrangementSelection::NoteClip {
                track_id: midi,
                clip_id: long_clip,
            },
            midi_target,
        ),
    ] {
        resident
            .song
            .drag(grabbed, |_| ArrangementMsg::MoveSelectedClips {
                anchor: grabbed.clip_id(),
                new_position_beats: 0.0,
                target_track,
            });
    }

    resident
        .engine
        .process_block(vibez_engine::engine::AudioProcessBlock::new(&mut [], 2));

    assert!(resident
        .source(audio_target)
        .clips
        .iter()
        .any(|clip| clip.id == loop_clip));
    assert!(resident
        .source(midi_target)
        .note_clips
        .iter()
        .any(|clip| clip.id == long_clip));
}
