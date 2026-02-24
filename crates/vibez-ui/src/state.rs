use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::constants::DEFAULT_BPM;
use vibez_core::effect::{EffectType, ParamDescriptor};
use vibez_core::id::{ClipId, EffectId, TrackId};
use vibez_core::midi::{MidiNote, TrackKind};

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
    // Looping
    pub loop_enabled: bool,
    pub loop_start: u64,
    pub loop_end: u64,
}

/// An effect instance as represented in the UI.
#[derive(Debug, Clone)]
pub struct UiEffect {
    pub id: EffectId,
    pub effect_type: EffectType,
    pub bypass: bool,
    pub params: Vec<f32>,
    pub descriptors: &'static [ParamDescriptor],
}

/// A note clip (MIDI pattern) as represented in the UI.
#[derive(Debug, Clone)]
pub struct UiNoteClip {
    pub id: ClipId,
    pub name: String,
    pub position_beats: f64,
    pub duration_beats: f64,
    pub notes: Vec<MidiNote>,
    pub selected_note: Option<usize>,
    // Looping
    pub loop_enabled: bool,
    pub loop_start_beats: f64,
    pub loop_end_beats: f64,
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
    pub effects: Vec<UiEffect>,
    pub note_clips: Vec<UiNoteClip>,
    pub kind: TrackKind,
    pub color_index: u8,
}

impl UiTrack {
    pub fn new(id: TrackId, name: String, color_index: u8) -> Self {
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
            effects: Vec::new(),
            note_clips: Vec::new(),
            kind: TrackKind::Audio,
            color_index,
        }
    }

    pub fn new_instrument(id: TrackId, name: String, kind: TrackKind, color_index: u8) -> Self {
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
            effects: Vec::new(),
            note_clips: Vec::new(),
            kind,
            color_index,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Workspace {
    Arrange,
    Mix,
}

/// Snap grid for piano roll quantization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapGrid {
    Quarter,
    Eighth,
    Sixteenth,
    ThirtySecond,
}

impl SnapGrid {
    /// Duration of one grid unit in beats.
    pub fn beat_size(self) -> f64 {
        match self {
            SnapGrid::Quarter => 1.0,
            SnapGrid::Eighth => 0.5,
            SnapGrid::Sixteenth => 0.25,
            SnapGrid::ThirtySecond => 0.125,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SnapGrid::Quarter => "1/4",
            SnapGrid::Eighth => "1/8",
            SnapGrid::Sixteenth => "1/16",
            SnapGrid::ThirtySecond => "1/32",
        }
    }

    pub fn all() -> &'static [SnapGrid] {
        &[
            SnapGrid::Quarter,
            SnapGrid::Eighth,
            SnapGrid::Sixteenth,
            SnapGrid::ThirtySecond,
        ]
    }

    /// Snap a beat value to the nearest grid position.
    pub fn snap_beat(self, beat: f64) -> f64 {
        let size = self.beat_size();
        (beat / size).round() * size
    }
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

    // Zoom / scroll (arrangement timeline)
    pub zoom_level: f32,
    pub scroll_offset_beats: f64,

    // Piano roll
    pub snap_grid: SnapGrid,

    // Multi-track
    pub tracks: Vec<UiTrack>,
    pub selected_track: Option<TrackId>,
    pub next_track_number: u32,

    // Detail panel: which note clip is selected for piano roll editing
    pub selected_note_clip: Option<(TrackId, ClipId)>,
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
            zoom_level: 1.0,
            scroll_offset_beats: 0.0,
            snap_grid: SnapGrid::Eighth,
            tracks: Vec::new(),
            selected_track: None,
            next_track_number: 1,
            selected_note_clip: None,
        }
    }
}

impl AppState {
    pub fn position_seconds(&self) -> f64 {
        self.position_samples as f64 / self.sample_rate as f64
    }

    pub fn position_beats(&self) -> f64 {
        self.position_seconds() * self.bpm / 60.0
    }

    pub fn duration_seconds(&self) -> f64 {
        let samples = self.total_duration_samples();
        if samples > 0 {
            samples as f64 / self.sample_rate as f64
        } else {
            0.0
        }
    }

    #[allow(dead_code)]
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

    /// Pixels per beat at the current zoom level.
    #[allow(dead_code)]
    pub fn pixels_per_beat(&self) -> f32 {
        20.0 * self.zoom_level
    }

    /// Number of beats visible in a canvas of the given width.
    #[allow(dead_code)]
    pub fn visible_beats(&self, canvas_width: f32) -> f64 {
        canvas_width as f64 / self.pixels_per_beat() as f64
    }

    /// Convert a beat value to a pixel x coordinate in the viewport.
    #[allow(dead_code)]
    pub fn beat_to_x(&self, beat: f64) -> f32 {
        ((beat - self.scroll_offset_beats) * self.pixels_per_beat() as f64) as f32
    }

    /// Convert a pixel x coordinate in the viewport to a beat value.
    #[allow(dead_code)]
    pub fn x_to_beat(&self, x: f32) -> f64 {
        x as f64 / self.pixels_per_beat() as f64 + self.scroll_offset_beats
    }

    /// Total duration in beats across all tracks.
    pub fn total_beats(&self) -> f64 {
        let dur = self.duration_seconds();
        if dur > 0.0 && self.bpm > 0.0 {
            dur * self.bpm / 60.0
        } else {
            // Minimum 16 beats to always show something useful
            16.0
        }
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
