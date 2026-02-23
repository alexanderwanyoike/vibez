use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::constants::DEFAULT_BPM;

pub struct AppState {
    // Audio data
    pub audio: Option<Arc<DecodedAudio>>,
    pub file_name: Option<String>,

    // Transport
    pub playing: bool,
    pub position_samples: u64,
    pub sample_rate: u32,

    // BPM
    pub bpm: f64,
    pub bpm_text: String,

    // Metering
    pub peak_l: f32,
    pub peak_r: f32,

    // UI
    pub loading: bool,
    pub status_text: String,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            audio: None,
            file_name: None,
            playing: false,
            position_samples: 0,
            sample_rate: 44_100,
            bpm: DEFAULT_BPM,
            bpm_text: format!("{DEFAULT_BPM:.0}"),
            peak_l: 0.0,
            peak_r: 0.0,
            loading: false,
            status_text: "Ready — Open a file to begin".to_string(),
        }
    }
}

impl AppState {
    pub fn position_seconds(&self) -> f64 {
        self.position_samples as f64 / self.sample_rate as f64
    }

    pub fn duration_seconds(&self) -> f64 {
        self.audio.as_ref().map_or(0.0, |a| a.duration_seconds())
    }

    pub fn position_normalized(&self) -> f64 {
        let dur = self.duration_seconds();
        if dur <= 0.0 {
            0.0
        } else {
            (self.position_seconds() / dur).clamp(0.0, 1.0)
        }
    }

    pub fn format_time(seconds: f64) -> String {
        let mins = (seconds / 60.0) as u32;
        let secs = seconds % 60.0;
        format!("{mins:02}:{secs:05.2}")
    }
}
