use std::path::PathBuf;
use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;

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

    // File
    OpenFile,
    FileSelected(Option<PathBuf>),
    AudioDecoded(Arc<DecodedAudio>),
    DecodeError(String),

    // Engine events (polled at 60fps)
    Tick,
    EnginePosition(u64),
    EngineMetering { peak_l: f32, peak_r: f32 },
    EngineStopped,
}
