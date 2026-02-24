use std::path::PathBuf;
use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::effect::EffectType;
use vibez_core::id::{ClipId, EffectId, TrackId};
use vibez_core::midi::MidiNote;

use crate::state::{SnapGrid, Workspace};

#[derive(Debug, Clone)]
pub enum Message {
    // Transport
    Play,
    Stop,
    TogglePlayback,
    Seek(f64),

    // BPM
    BpmChanged(String),
    BpmSubmit,

    // Workspace
    SwitchWorkspace(Workspace),

    // Engine events
    Tick,
    EnginePosition(u64),
    EngineMetering {
        peak_l: f32,
        peak_r: f32,
    },
    EngineStopped,

    // Multi-track
    AddTrack,
    RemoveTrack(TrackId),
    SelectTrack(TrackId),
    AddClipToTrack(TrackId),
    ClipFileSelected(TrackId, Option<PathBuf>),
    ClipAudioDecoded(TrackId, ClipId, Arc<DecodedAudio>, String),
    ClipDecodeError(TrackId, String),
    RemoveClip(TrackId, ClipId),

    // Track controls
    SetTrackGain(TrackId, f32),
    SetTrackPan(TrackId, f32),
    SetTrackMute(TrackId),
    SetTrackSolo(TrackId),

    // Per-track metering
    EngineTrackMeter {
        track_id: TrackId,
        peak_l: f32,
        peak_r: f32,
    },

    // Effects
    AddEffect(TrackId, EffectType),
    RemoveEffect(TrackId, EffectId),
    SetEffectParam(TrackId, EffectId, usize, f32),
    ToggleEffectBypass(TrackId, EffectId),
    MoveEffectUp(TrackId, EffectId),
    MoveEffectDown(TrackId, EffectId),

    // Instrument tracks
    AddInstrumentTrack,
    SetSynthParam(TrackId, usize, f32),

    // Zoom / scroll
    ZoomIn,
    ZoomOut,
    SetZoom(f32),
    ScrollArrangement(f64),

    // Snap grid
    SetSnapGrid(SnapGrid),

    // Clip looping
    ToggleClipLoop(TrackId, ClipId),
    SetClipLoopRegion {
        track_id: TrackId,
        clip_id: ClipId,
        loop_start: u64,
        loop_end: u64,
    },
    ToggleNoteClipLoop(TrackId, ClipId),
    SetNoteClipLoopRegion {
        track_id: TrackId,
        clip_id: ClipId,
        loop_start_beats: f64,
        loop_end_beats: f64,
    },

    // Piano roll / note clips
    AddNoteClipToTrack(TrackId),
    SelectNoteClip(TrackId, ClipId),
    AddNote {
        track_id: TrackId,
        clip_id: ClipId,
        pitch: u8,
        start_beat: f64,
        duration_beats: f64,
    },
    RemoveNote(TrackId, ClipId, usize),
    EditNote(TrackId, ClipId, usize, MidiNote),
    SelectNote(TrackId, ClipId, Option<usize>),
}
