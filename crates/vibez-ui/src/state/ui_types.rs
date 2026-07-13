//! UI-side data types for tracks, clips, devices, and grids.

use super::default_drum_rack_pads;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::effect::{EffectType, ParamDescriptor};
use vibez_core::id::{ClipId, EffectId, TrackId};
use vibez_core::midi::{InstrumentKind, MidiNote, TrackKind};
use vibez_core::track::{DrumPadState, MediaSourceRef};

/// A clip as represented in the UI.
#[derive(Debug, Clone)]
pub struct UiClip {
    pub id: ClipId,
    pub name: String,
    pub audio: Arc<DecodedAudio>,
    pub source: Option<MediaSourceRef>,
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
    /// Nominal BPM of the underlying sample. `None` until detected or
    /// entered manually.
    pub original_bpm: Option<f64>,
    /// Whether `audio` has been time-stretched to fit the project
    /// tempo.
    pub warped: bool,
    /// Project BPM the current warped audio was stretched to. Used to
    /// flag staleness in the timeline when the project tempo changes.
    pub warped_to_bpm: Option<f64>,
    /// Un-warped source audio, retained so the UI can re-warp to a new
    /// project BPM or clear the warp without re-decoding. Populated on
    /// import or on first warp. Not persisted: on reload the UI
    /// re-decodes from the source and re-warps.
    pub original_audio: Option<Arc<DecodedAudio>>,
}

#[derive(Debug, Clone)]
pub struct UiDrumPad {
    pub name: Option<String>,
    pub source: Option<MediaSourceRef>,
    /// Decoded audio kept on the UI side so offline bounce can re-seed a
    /// drum rack without a round-trip through the audio thread.
    pub audio: Option<Arc<DecodedAudio>>,
    pub gain: f32,
    pub pan: f32,
    pub start: f32,
    pub end: f32,
    pub coarse_tune: i8,
    pub fine_tune: f32,
    pub one_shot: bool,
    pub choke_group: Option<u8>,
}

impl Default for UiDrumPad {
    fn default() -> Self {
        Self {
            name: None,
            source: None,
            audio: None,
            gain: 1.0,
            pan: 0.0,
            start: 0.0,
            end: 1.0,
            coarse_tune: 0,
            fine_tune: 0.0,
            one_shot: true,
            choke_group: None,
        }
    }
}

impl UiDrumPad {
    pub fn to_state(&self) -> DrumPadState {
        DrumPadState {
            source: self.source.clone(),
            gain: self.gain,
            pan: self.pan,
            start: self.start,
            end: self.end,
            coarse_tune: self.coarse_tune,
            fine_tune: self.fine_tune,
            one_shot: self.one_shot,
            choke_group: self.choke_group,
        }
    }

    pub fn from_state(state: &DrumPadState) -> Self {
        Self {
            name: state.source.as_ref().map(MediaSourceRef::display_name),
            source: state.source.clone(),
            audio: None,
            gain: state.gain,
            pan: state.pan,
            start: state.start,
            end: state.end,
            coarse_tune: state.coarse_tune,
            fine_tune: state.fine_tune,
            one_shot: state.one_shot,
            choke_group: state.choke_group,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SampleBrowserEntry {
    pub source: MediaSourceRef,
    pub name: String,
    pub root_path: PathBuf,
    pub relative_path: PathBuf,
    pub format: String,
    /// Derived only after the shared decoder validates/materializes the source.
    pub duration_seconds: Option<f64>,
    pub channels: Option<usize>,
    pub sample_rate: Option<u32>,
    pub file_size: Option<u64>,
    pub modified: Option<SystemTime>,
    pub search_text: String,
}

/// A read-only folder record in the Local Catalog. Roots remain configuration;
/// these records only describe Source Storage for navigation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SampleBrowserFolder {
    pub path: PathBuf,
    pub root_path: PathBuf,
    pub relative_path: PathBuf,
    pub name: String,
    pub search_text: String,
}

/// An effect instance as represented in the UI.
#[derive(Debug, Clone)]
pub struct UiEffect {
    pub id: EffectId,
    pub effect_type: EffectType,
    pub bypass: bool,
    pub params: Vec<f32>,
    pub descriptors: &'static [ParamDescriptor],
    /// Display name override for external plugins.
    pub plugin_name: Option<String>,
    /// Whether this effect has a native plugin GUI available.
    pub has_plugin_gui: bool,
    /// Persistent identity of the plugin backing this slot, if any.
    pub plugin_ref: Option<vibez_core::effect::PluginDeviceInfo>,
}

/// A note clip (MIDI pattern) as represented in the UI.
#[derive(Debug, Clone)]
pub struct UiNoteClip {
    pub id: ClipId,
    pub name: String,
    pub position_beats: f64,
    pub duration_beats: f64,
    pub notes: Vec<MidiNote>,
    pub selected_notes: HashSet<usize>,
    // Looping
    pub loop_enabled: bool,
    pub loop_start_beats: f64,
    pub loop_end_beats: f64,
}

#[derive(Debug, Clone)]
pub enum ClipboardClip {
    Audio {
        track_id: TrackId,
        offset_beats: f64,
        clip: UiClip,
    },
    Note {
        track_id: TrackId,
        offset_beats: f64,
        clip: UiNoteClip,
    },
}

#[derive(Debug, Clone, Default)]
pub struct ClipClipboard {
    pub clips: Vec<ClipboardClip>,
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
    /// Post-fader send amounts into buses: `(bus id, 0..1)`.
    pub sends: Vec<(TrackId, f32)>,
    pub note_clips: Vec<UiNoteClip>,
    pub kind: TrackKind,
    pub color_index: u8,
    pub has_instrument: bool,
    pub instrument_kind: Option<InstrumentKind>,
    pub sample_name: Option<String>,
    pub sample_source: Option<MediaSourceRef>,
    /// Decoded audio for the sampler, kept UI-side so offline bounce can
    /// re-seed a fresh sampler instance.
    pub sample_audio: Option<Arc<DecodedAudio>>,
    pub instrument_params: Vec<f32>,
    pub drum_rack_pads: Vec<UiDrumPad>,
    /// Automation lanes (mirrored to the engine like note clips).
    pub automation: Vec<vibez_core::automation::AutomationLane>,
    pub selected_drum_pad: usize,
    /// Display name for external plugin instruments (e.g. "Dexed", "Surge XT").
    pub plugin_instrument_name: Option<String>,
    /// Persistent identity of the plugin instrument, if any.
    pub plugin_instrument_ref: Option<vibez_core::effect::PluginDeviceInfo>,
    /// Parameters of the plugin instrument (leaked 'static by the
    /// wrapper); empty for built-in instruments.
    pub plugin_instrument_descriptors: &'static [vibez_core::effect::ParamDescriptor],
    /// Whether the plugin instrument has a native GUI.
    pub has_plugin_instrument_gui: bool,
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
            sends: Vec::new(),
            note_clips: Vec::new(),
            kind: TrackKind::Audio,
            color_index,
            has_instrument: false,
            instrument_kind: None,
            sample_name: None,
            sample_source: None,
            sample_audio: None,
            instrument_params: Vec::new(),
            drum_rack_pads: default_drum_rack_pads(),
            automation: Vec::new(),
            selected_drum_pad: 0,
            plugin_instrument_name: None,
            plugin_instrument_ref: None,
            plugin_instrument_descriptors: &[],
            has_plugin_instrument_gui: false,
        }
    }

    pub fn new_instrument(id: TrackId, name: String, kind: TrackKind, color_index: u8) -> Self {
        let (has_instrument, instrument_kind) = match kind {
            TrackKind::Instrument(ik) => (true, Some(ik)),
            _ => (false, None),
        };
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
            sends: Vec::new(),
            note_clips: Vec::new(),
            kind,
            color_index,
            has_instrument,
            instrument_kind,
            sample_name: None,
            sample_source: None,
            sample_audio: None,
            instrument_params: Vec::new(),
            drum_rack_pads: default_drum_rack_pads(),
            automation: Vec::new(),
            selected_drum_pad: 0,
            plugin_instrument_name: None,
            plugin_instrument_ref: None,
            plugin_instrument_descriptors: &[],
            has_plugin_instrument_gui: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArrangementSelection {
    AudioClip { track_id: TrackId, clip_id: ClipId },
    NoteClip { track_id: TrackId, clip_id: ClipId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Workspace {
    Arrange,
    Mix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailPanelTab {
    Clip,
    Devices,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GridDivision {
    EightBars,
    FourBars,
    TwoBars,
    Bar,
    Quarter,
    Eighth,
    Sixteenth,
    ThirtySecond,
}

/// A musical grid division, optionally interpreted as a triplet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapGrid {
    division: GridDivision,
    triplet: bool,
}

impl SnapGrid {
    pub const EIGHT_BARS: Self = Self::straight(GridDivision::EightBars);
    pub const FOUR_BARS: Self = Self::straight(GridDivision::FourBars);
    pub const TWO_BARS: Self = Self::straight(GridDivision::TwoBars);
    pub const BAR: Self = Self::straight(GridDivision::Bar);
    pub const QUARTER: Self = Self::straight(GridDivision::Quarter);
    pub const EIGHTH: Self = Self::straight(GridDivision::Eighth);
    pub const SIXTEENTH: Self = Self::straight(GridDivision::Sixteenth);
    pub const THIRTY_SECOND: Self = Self::straight(GridDivision::ThirtySecond);
    const ALL: [Self; 12] = [
        Self::EIGHT_BARS,
        Self::FOUR_BARS,
        Self::TWO_BARS,
        Self::BAR,
        Self::QUARTER,
        Self::EIGHTH,
        Self::SIXTEENTH,
        Self::THIRTY_SECOND,
        Self::QUARTER.triplet(),
        Self::EIGHTH.triplet(),
        Self::SIXTEENTH.triplet(),
        Self::THIRTY_SECOND.triplet(),
    ];

    const fn straight(division: GridDivision) -> Self {
        Self {
            division,
            triplet: false,
        }
    }

    /// Duration of one grid unit in beats.
    pub fn beat_size(self) -> f64 {
        let straight = match self.division {
            GridDivision::EightBars => 32.0,
            GridDivision::FourBars => 16.0,
            GridDivision::TwoBars => 8.0,
            GridDivision::Bar => 4.0,
            GridDivision::Quarter => 1.0,
            GridDivision::Eighth => 0.5,
            GridDivision::Sixteenth => 0.25,
            GridDivision::ThirtySecond => 0.125,
        };
        if self.triplet {
            straight * 2.0 / 3.0
        } else {
            straight
        }
    }

    pub fn label(self) -> &'static str {
        match (self.division, self.triplet) {
            (GridDivision::EightBars, false) => "8 Bars",
            (GridDivision::FourBars, false) => "4 Bars",
            (GridDivision::TwoBars, false) => "2 Bars",
            (GridDivision::Bar, false) => "1 Bar",
            (GridDivision::Quarter, false) => "1/4",
            (GridDivision::Eighth, false) => "1/8",
            (GridDivision::Sixteenth, false) => "1/16",
            (GridDivision::ThirtySecond, false) => "1/32",
            (GridDivision::EightBars, true) => "8 Bars T",
            (GridDivision::FourBars, true) => "4 Bars T",
            (GridDivision::TwoBars, true) => "2 Bars T",
            (GridDivision::Bar, true) => "1 Bar T",
            (GridDivision::Quarter, true) => "1/4T",
            (GridDivision::Eighth, true) => "1/8T",
            (GridDivision::Sixteenth, true) => "1/16T",
            (GridDivision::ThirtySecond, true) => "1/32T",
        }
    }

    pub fn all() -> &'static [SnapGrid] {
        &Self::ALL
    }

    pub const fn triplet(self) -> Self {
        Self {
            triplet: true,
            ..self
        }
    }

    pub const fn toggle_triplet(self) -> Self {
        Self {
            triplet: !self.triplet,
            ..self
        }
    }

    pub const fn is_triplet(self) -> bool {
        self.triplet
    }

    pub fn narrower(self) -> Self {
        Self {
            division: match self.division {
                GridDivision::EightBars => GridDivision::FourBars,
                GridDivision::FourBars => GridDivision::TwoBars,
                GridDivision::TwoBars => GridDivision::Bar,
                GridDivision::Bar => GridDivision::Quarter,
                GridDivision::Quarter => GridDivision::Eighth,
                GridDivision::Eighth => GridDivision::Sixteenth,
                GridDivision::Sixteenth | GridDivision::ThirtySecond => GridDivision::ThirtySecond,
            },
            ..self
        }
    }

    pub fn wider(self) -> Self {
        Self {
            division: match self.division {
                GridDivision::EightBars | GridDivision::FourBars => GridDivision::EightBars,
                GridDivision::TwoBars => GridDivision::FourBars,
                GridDivision::Bar => GridDivision::TwoBars,
                GridDivision::Quarter => GridDivision::Bar,
                GridDivision::Eighth => GridDivision::Quarter,
                GridDivision::Sixteenth => GridDivision::Eighth,
                GridDivision::ThirtySecond => GridDivision::Sixteenth,
            },
            ..self
        }
    }

    pub fn adaptive(pixels_per_beat: f32, bias: i8, triplet: bool) -> Self {
        let divisions = [
            Self::EIGHT_BARS,
            Self::FOUR_BARS,
            Self::TWO_BARS,
            Self::BAR,
            Self::QUARTER,
            Self::EIGHTH,
            Self::SIXTEENTH,
            Self::THIRTY_SECOND,
        ];
        let base = divisions
            .iter()
            .rposition(|grid| grid.beat_size() * pixels_per_beat as f64 >= 12.0)
            .unwrap_or(0);
        let index = (base as i8 + bias).clamp(0, divisions.len() as i8 - 1) as usize;
        let grid = divisions[index];
        if triplet {
            grid.triplet()
        } else {
            grid
        }
    }

    /// Snap a beat value to the nearest grid position.
    pub fn snap_beat(self, beat: f64) -> f64 {
        let size = self.beat_size();
        (beat / size).round() * size
    }
}

impl std::fmt::Display for SnapGrid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Shared grid modes passed into arrangement and MIDI editor widgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridConfig {
    pub selected: SnapGrid,
    pub snap_enabled: bool,
    pub adaptive: bool,
    pub adaptive_bias: i8,
}

impl GridConfig {
    pub const fn new(
        selected: SnapGrid,
        snap_enabled: bool,
        adaptive: bool,
        adaptive_bias: i8,
    ) -> Self {
        Self {
            selected,
            snap_enabled,
            adaptive,
            adaptive_bias,
        }
    }

    pub fn effective_grid(self, pixels_per_beat: f32) -> SnapGrid {
        if self.adaptive {
            SnapGrid::adaptive(
                pixels_per_beat,
                self.adaptive_bias,
                self.selected.is_triplet(),
            )
        } else {
            self.selected
        }
    }

    pub fn snap_beat(self, beat: f64, pixels_per_beat: f32) -> f64 {
        if self.snap_enabled {
            self.effective_grid(pixels_per_beat).snap_beat(beat)
        } else {
            beat
        }
    }
}

#[cfg(test)]
mod snap_grid_tests {
    use super::*;

    #[test]
    fn supports_bars_subdivisions_and_triplets() {
        assert_eq!(SnapGrid::EIGHT_BARS.beat_size(), 32.0);
        assert_eq!(SnapGrid::FOUR_BARS.beat_size(), 16.0);
        assert_eq!(SnapGrid::BAR.beat_size(), 4.0);
        assert_eq!(SnapGrid::EIGHTH.beat_size(), 0.5);
        assert!((SnapGrid::EIGHTH.triplet().beat_size() - 1.0 / 3.0).abs() < 1e-9);
        assert_eq!(SnapGrid::EIGHTH.triplet().label(), "1/8T");
    }

    #[test]
    fn narrower_and_wider_preserve_triplet_mode() {
        assert_eq!(SnapGrid::EIGHT_BARS.narrower(), SnapGrid::FOUR_BARS);
        assert_eq!(SnapGrid::FOUR_BARS.wider(), SnapGrid::EIGHT_BARS);
        assert_eq!(SnapGrid::BAR.narrower(), SnapGrid::QUARTER);
        assert_eq!(SnapGrid::QUARTER.wider(), SnapGrid::BAR);
        assert_eq!(
            SnapGrid::EIGHTH.triplet().narrower(),
            SnapGrid::SIXTEENTH.triplet()
        );
        assert_eq!(SnapGrid::THIRTY_SECOND.narrower(), SnapGrid::THIRTY_SECOND);
        assert_eq!(SnapGrid::EIGHT_BARS.wider(), SnapGrid::EIGHT_BARS);
    }

    #[test]
    fn adaptive_grid_gets_narrower_as_pixels_per_beat_increase() {
        assert_eq!(SnapGrid::adaptive(0.25, 0, false), SnapGrid::EIGHT_BARS);
        assert_eq!(SnapGrid::adaptive(5.0, 0, false), SnapGrid::BAR);
        assert_eq!(SnapGrid::adaptive(20.0, 0, false), SnapGrid::QUARTER);
        assert_eq!(SnapGrid::adaptive(80.0, 0, false), SnapGrid::SIXTEENTH);
        assert_eq!(
            SnapGrid::adaptive(80.0, 0, true),
            SnapGrid::SIXTEENTH.triplet()
        );
    }

    #[test]
    fn grid_config_resolves_adaptive_density_and_can_disable_snapping() {
        let fixed = GridConfig::new(SnapGrid::EIGHTH, true, false, 0);
        assert_eq!(fixed.effective_grid(80.0), SnapGrid::EIGHTH);
        assert_eq!(fixed.snap_beat(0.31, 80.0), 0.5);

        let adaptive = GridConfig::new(SnapGrid::EIGHTH.triplet(), true, true, 0);
        assert_eq!(adaptive.effective_grid(80.0), SnapGrid::SIXTEENTH.triplet());

        let free = GridConfig::new(SnapGrid::SIXTEENTH, false, false, 0);
        assert_eq!(free.snap_beat(0.31, 80.0), 0.31);
    }
}
