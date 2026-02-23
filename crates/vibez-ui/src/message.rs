use std::path::PathBuf;
use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::{ClipId, TrackId};

#[derive(Debug, Clone)]
pub enum Message {
    // Transport
    Play,
    Stop,
    TogglePlayback,
    Seek(f64), // 0.0..1.0 normalized position

    // BPM
    BpmChanged(String),
    BpmSubmit,

    // Open file → creates audio track + clip
    OpenFileToNewTrack,
    NewTrackFileSelected(Option<PathBuf>),
    NewTrackAudioDecoded(TrackId, ClipId, Arc<DecodedAudio>, String),
    NewTrackDecodeError(String),

    // Engine events (polled at 60fps)
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
}
