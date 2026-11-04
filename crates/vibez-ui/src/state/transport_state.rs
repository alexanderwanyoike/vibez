/// Transport domain state: playback position, tempo, and the
/// arrangement loop. First extracted domain slice of the
/// architecture refactor: owns everything the transport bar and the
/// beat/sample math need, and nothing else.
#[derive(Debug)]
pub struct TransportState {
    /// What the engine last reported, so the screen never runs ahead of it.
    pub playing: bool,
    /// A Play or Stop the engine has not confirmed yet.
    pub requested_playing: Option<bool>,
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
            playing: false,
            requested_playing: None,
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
