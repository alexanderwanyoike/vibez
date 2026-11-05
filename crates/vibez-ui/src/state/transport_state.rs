/// The engine's last reported transport, with any Play or Stop it has not
/// confirmed yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackPhase {
    Stopped,
    /// Play was sent and the engine has not started yet.
    Starting,
    Playing,
    /// Stop was sent and the engine has not stopped yet.
    Stopping,
}

/// Transport domain state: playback position, tempo, and the
/// arrangement loop. First extracted domain slice of the
/// architecture refactor: owns everything the transport bar and the
/// beat/sample math need, and nothing else.
#[derive(Debug)]
pub struct TransportState {
    pub playback: PlaybackPhase,
    pub position_samples: u64,
    pub sample_rate: u32,
    pub bpm: f64,
    pub bpm_text: String,
    pub loop_enabled: bool,
    pub loop_start_beats: f64,
    pub loop_end_beats: f64,
}

impl Default for TransportState {
    fn default() -> Self {
        Self {
            playback: PlaybackPhase::Stopped,
            position_samples: 0,
            sample_rate: 44_100,
            bpm: 120.0,
            bpm_text: "120".to_string(),
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 4.0,
        }
    }
}
