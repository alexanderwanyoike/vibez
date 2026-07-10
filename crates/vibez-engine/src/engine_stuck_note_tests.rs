//! Stuck-note regression tests: every note-on must see its
//! note-off across clip/loop/transport boundaries.

use super::*;
use std::sync::{Arc, Mutex};
use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::{InstrumentKind, MidiNote};

/// Instrument that records every note event it receives.
struct SpyInstrument {
    events: Arc<Mutex<Vec<(bool, u8)>>>,
}

impl vibez_instruments::Instrument for SpyInstrument {
    fn instrument_kind(&self) -> InstrumentKind {
        InstrumentKind::SubtractiveSynth
    }
    fn param_descriptors(&self) -> &'static [vibez_core::effect::ParamDescriptor] {
        &[]
    }
    fn set_param(&mut self, _index: usize, _value: f32) -> bool {
        false
    }
    fn get_param(&self, _index: usize) -> f32 {
        0.0
    }
    fn note_on(&mut self, pitch: u8, _velocity: u8) {
        self.events.lock().unwrap().push((true, pitch));
    }
    fn note_off(&mut self, pitch: u8) {
        self.events.lock().unwrap().push((false, pitch));
    }
    fn render(&mut self, _buffer: &mut [f32], _channels: usize) {}
    fn reset(&mut self) {}
}

/// Engine with one MIDI track holding a held note (0..8 beats in
/// an 8-beat clip, no clip loop) and a spy instrument.
fn engine_with_held_note() -> (
    AudioEngine,
    rtrb::Producer<EngineCommand>,
    Arc<Mutex<Vec<(bool, u8)>>>,
    TrackId,
) {
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let events = Arc::new(Mutex::new(Vec::new()));
    let tid = TrackId::new();
    let cid = ClipId::new();
    cmd_tx
        .push(EngineCommand::AddMidiTrack(tid, "midi".into()))
        .unwrap();
    cmd_tx
        .push(EngineCommand::SetPluginInstrument {
            track_id: tid,
            instrument: Box::new(SpyInstrument {
                events: Arc::clone(&events),
            }),
        })
        .unwrap();
    cmd_tx
        .push(EngineCommand::AddNoteClip {
            track_id: tid,
            clip_id: cid,
            position_beats: 0.0,
            duration_beats: 8.0,
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
        })
        .unwrap();
    cmd_tx
        .push(EngineCommand::AddNote {
            track_id: tid,
            clip_id: cid,
            note: MidiNote {
                pitch: 60,
                velocity: 100,
                start_beat: 0.0,
                duration_beats: 8.0,
            },
        })
        .unwrap();
    cmd_tx.push(EngineCommand::Play).unwrap();
    let mut buf = vec![0.0f32; 512 * 2];
    engine.process(&mut buf, 2); // drains commands, starts the note
    assert!(
        events.lock().unwrap().contains(&(true, 60)),
        "note-on should have fired"
    );
    (engine, cmd_tx, events, tid)
}

/// Reproduce the dogfood report: 8-beat clip, 1-beat note at the
/// start, clip loop on, arrangement loop over the same 2 bars.
/// Every note-on must be closed by a note-off before the next
/// note-on of the same pitch (otherwise the voice piles up /
/// drones).
fn assert_no_hanging_notes(events: &[(bool, u8)]) {
    let mut sounding = false;
    for (i, &(is_on, pitch)) in events.iter().enumerate() {
        assert_eq!(pitch, 60);
        if is_on {
            assert!(!sounding, "note-on #{i} while already sounding: {events:?}");
            sounding = true;
        } else {
            sounding = false;
        }
    }
}

fn run_scenario(
    clip_loop: (bool, f64, f64),
    arr_loop: Option<(u64, u64)>,
    note_dur: f64,
    blocks: usize,
) -> Vec<(bool, u8)> {
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let events = Arc::new(Mutex::new(Vec::new()));
    let tid = TrackId::new();
    let cid = ClipId::new();
    cmd_tx
        .push(EngineCommand::AddMidiTrack(tid, "midi".into()))
        .unwrap();
    cmd_tx
        .push(EngineCommand::SetPluginInstrument {
            track_id: tid,
            instrument: Box::new(SpyInstrument {
                events: Arc::clone(&events),
            }),
        })
        .unwrap();
    cmd_tx
        .push(EngineCommand::AddNoteClip {
            track_id: tid,
            clip_id: cid,
            position_beats: 0.0,
            duration_beats: 8.0,
            loop_enabled: clip_loop.0,
            loop_start_beats: clip_loop.1,
            loop_end_beats: clip_loop.2,
        })
        .unwrap();
    cmd_tx
        .push(EngineCommand::AddNote {
            track_id: tid,
            clip_id: cid,
            note: MidiNote {
                pitch: 60,
                velocity: 100,
                start_beat: 0.0,
                duration_beats: note_dur,
            },
        })
        .unwrap();
    if let Some((start, end)) = arr_loop {
        cmd_tx
            .push(EngineCommand::SetArrangementLoopRegion { start, end })
            .unwrap();
        cmd_tx
            .push(EngineCommand::SetArrangementLoop(true))
            .unwrap();
    }
    cmd_tx.push(EngineCommand::Play).unwrap();
    let mut buf = vec![0.0f32; 512 * 2];
    for _ in 0..blocks {
        engine.process(&mut buf, 2);
    }
    let out = events.lock().unwrap().clone();
    out
}

#[test]
fn dogfood_clip_loop_one_bar_in_two_bar_clip() {
    // Clip loop repeats the first bar inside the 2-bar clip.
    // 120 BPM default: samples_per_beat = 44100 * 60/120 = 22050.
    // 8 beats = 176400 samples. Arrangement loop the same 2 bars.
    let events = run_scenario((true, 0.0, 4.0), Some((0, 176_400)), 1.0, 1500);
    assert!(
        events.iter().filter(|e| e.0).count() >= 4,
        "expected several note-ons: {events:?}"
    );
    assert_no_hanging_notes(&events);
}

#[test]
fn dogfood_full_clip_loop_with_arrangement_loop() {
    let events = run_scenario((true, 0.0, 8.0), Some((0, 176_400)), 1.0, 1500);
    assert!(
        events.iter().filter(|e| e.0).count() >= 2,
        "expected repeated note-ons: {events:?}"
    );
    assert_no_hanging_notes(&events);
}

#[test]
fn dogfood_note_spanning_clip_loop_boundary() {
    // Note as long as the loop region: off lands exactly on the wrap.
    let events = run_scenario((true, 0.0, 4.0), Some((0, 176_400)), 4.0, 1500);
    assert_no_hanging_notes(&events);
}

#[test]
fn instrument_params_change_the_sound() {
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let tid = TrackId::new();
    cmd_tx
        .push(EngineCommand::AddMidiTrack(tid, "midi".into()))
        .unwrap();
    cmd_tx
        .push(EngineCommand::SetTrackInstrument(
            tid,
            vibez_core::midi::InstrumentKind::SubtractiveSynth,
        ))
        .unwrap();
    cmd_tx
        .push(EngineCommand::AuditionNote {
            track_id: tid,
            pitch: 60,
            velocity: 100,
            on: true,
        })
        .unwrap();
    let mut open_buf = vec![0.0f32; 2048 * 2];
    engine.process(&mut open_buf, 2);
    engine.process(&mut open_buf, 2);

    // Slam the filter shut (param 5 = cutoff in Hz) and compare.
    cmd_tx
        .push(EngineCommand::SetInstrumentParam {
            track_id: tid,
            param_index: 5,
            value: 60.0,
        })
        .unwrap();
    let mut closed_buf = vec![0.0f32; 2048 * 2];
    engine.process(&mut closed_buf, 2);
    engine.process(&mut closed_buf, 2);

    let rms = |b: &[f32]| (b.iter().map(|s| s * s).sum::<f32>() / b.len() as f32).sqrt();
    let open_rms = rms(&open_buf);
    let closed_rms = rms(&closed_buf);
    assert!(open_rms > 0.0);
    assert!(
        closed_rms < open_rms * 0.7,
        "60 Hz cutoff must audibly darken/quiet a C4 saw: open={open_rms} closed={closed_rms}"
    );
}

#[test]
fn audition_sounds_while_transport_stopped() {
    let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
    let tid = TrackId::new();
    cmd_tx
        .push(EngineCommand::AddMidiTrack(tid, "midi".into()))
        .unwrap();
    cmd_tx
        .push(EngineCommand::SetTrackInstrument(
            tid,
            vibez_core::midi::InstrumentKind::SubtractiveSynth,
        ))
        .unwrap();
    cmd_tx
        .push(EngineCommand::AuditionNote {
            track_id: tid,
            pitch: 60,
            velocity: 100,
            on: true,
        })
        .unwrap();
    // Transport NEVER started: idle rendering must still sound.
    let mut buf = vec![0.0f32; 512 * 2];
    engine.process(&mut buf, 2);
    engine.process(&mut buf, 2);
    assert!(
        buf.iter().any(|&s| s.abs() > 1e-6),
        "auditioned note must be audible while stopped"
    );
    cmd_tx
        .push(EngineCommand::AuditionNote {
            track_id: tid,
            pitch: 60,
            velocity: 100,
            on: false,
        })
        .unwrap();
    // Long tail drain: after release the synth decays to silence.
    for _ in 0..200 {
        engine.process(&mut buf, 2);
    }
    assert!(
        buf.iter().all(|&s| s.abs() < 1e-4),
        "note must decay after audition release"
    );
}

#[test]
fn removing_clip_kills_sounding_notes() {
    let (mut engine, mut cmd_tx, events, tid) = engine_with_held_note();
    // Clip id is unknown here; removing ALL note clips on the
    // track exercises the same path.
    let cid = {
        // recover clip id from engine state
        engine.tracks()[0].note_clips[0].id
    };
    cmd_tx
        .push(EngineCommand::RemoveNoteClip(tid, cid))
        .unwrap();
    let mut buf = vec![0.0f32; 512 * 2];
    engine.process(&mut buf, 2);
    assert!(
        events.lock().unwrap().contains(&(false, 60)),
        "deleting the clip must kill its sounding notes"
    );
}

#[test]
fn arrangement_loop_wrap_kills_sounding_notes() {
    let (mut engine, mut cmd_tx, events, _tid) = engine_with_held_note();
    // Tight arrangement loop so the transport wraps mid-note.
    cmd_tx
        .push(EngineCommand::SetArrangementLoopRegion {
            start: 0,
            end: 2048,
        })
        .unwrap();
    cmd_tx
        .push(EngineCommand::SetArrangementLoop(true))
        .unwrap();

    let mut buf = vec![0.0f32; 512 * 2];
    for _ in 0..8 {
        engine.process(&mut buf, 2); // crosses 2048 and wraps
    }
    assert!(
        events.lock().unwrap().contains(&(false, 60)),
        "wrap must send a note-off for the sounding note, got {:?}",
        events.lock().unwrap()
    );
}

#[test]
fn midi_only_arrangement_loop_retriggers_at_loop_start() {
    let events = run_scenario((false, 0.0, 0.0), Some((0, 2048)), 8.0, 12);
    let note_ons = events
        .iter()
        .filter(|(on, pitch)| *on && *pitch == 60)
        .count();
    assert!(
        note_ons >= 2,
        "MIDI-only arrangement loop must retrigger its first note: {events:?}"
    );
}

#[test]
fn stop_kills_sounding_notes() {
    let (mut engine, mut cmd_tx, events, _tid) = engine_with_held_note();
    cmd_tx.push(EngineCommand::Stop).unwrap();
    let mut buf = vec![0.0f32; 512 * 2];
    engine.process(&mut buf, 2);
    assert!(
        events.lock().unwrap().contains(&(false, 60)),
        "stop must send a note-off for the sounding note"
    );
}

#[test]
fn seek_kills_sounding_notes() {
    let (mut engine, mut cmd_tx, events, _tid) = engine_with_held_note();
    cmd_tx.push(EngineCommand::Seek(96_000)).unwrap();
    let mut buf = vec![0.0f32; 512 * 2];
    engine.process(&mut buf, 2);
    assert!(
        events.lock().unwrap().contains(&(false, 60)),
        "seek must send a note-off for the sounding note"
    );
}
