use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::constants::DEFAULT_BPM;
use vibez_core::id::{ClipId, TrackId};

/// A clip as represented in the UI.
#[derive(Debug, Clone)]
pub struct UiClip {
    pub id: ClipId,
    pub name: String,
    pub audio: Arc<DecodedAudio>,
    /// Position on the timeline in samples.
    pub position: u64,
    /// Offset into the source audio in samples.
    pub source_offset: u64,
    /// Duration in samples.
    pub duration: u64,
}

/// A track as represented in the UI.
#[derive(Debug, Clone)]
pub struct UiTrack {
    pub id: TrackId,
    pub name: String,
    pub clips: Vec<UiClip>,
    pub gain: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    pub peak_l: f32,
    pub peak_r: f32,
}

impl UiTrack {
    pub fn new(id: TrackId, name: String) -> Self {
        Self {
            id,
            name,
            clips: Vec::new(),
            gain: 1.0,
            pan: 0.5,
            mute: false,
            solo: false,
            peak_l: 0.0,
            peak_r: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Workspace {
    Arrange,
    Mix,
}

pub struct AppState {
    // Transport
    pub playing: bool,
    pub position_samples: u64,
    pub sample_rate: u32,

    // BPM
    pub bpm: f64,
    pub bpm_text: String,

    // Metering (master)
    pub peak_l: f32,
    pub peak_r: f32,

    // UI
    pub status_text: String,
    pub workspace: Workspace,

    // Multi-track
    pub tracks: Vec<UiTrack>,
    pub selected_track: Option<TrackId>,
    pub next_track_number: u32,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            playing: false,
            position_samples: 0,
            sample_rate: 44_100,
            bpm: DEFAULT_BPM,
            bpm_text: format!("{DEFAULT_BPM:.0}"),
            peak_l: 0.0,
            peak_r: 0.0,
            status_text: "Ready — Add a track to get started".to_string(),
            workspace: Workspace::Arrange,
            tracks: Vec::new(),
            selected_track: None,
            next_track_number: 1,
        }
    }
}

impl AppState {
    pub fn position_seconds(&self) -> f64 {
        self.position_samples as f64 / self.sample_rate as f64
    }

    pub fn duration_seconds(&self) -> f64 {
        let samples = self.total_duration_samples();
        if samples > 0 {
            samples as f64 / self.sample_rate as f64
        } else {
            0.0
        }
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

    pub fn find_track(&self, id: TrackId) -> Option<&UiTrack> {
        self.tracks.iter().find(|t| t.id == id)
    }

    pub fn find_track_mut(&mut self, id: TrackId) -> Option<&mut UiTrack> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    /// Total duration in samples across all tracks (max clip end position).
    pub fn total_duration_samples(&self) -> u64 {
        self.tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .map(|c| c.position.saturating_add(c.duration))
            .max()
            .unwrap_or(0)
    }
}
