//! UI-side data types for tracks, clips, devices, and grids.

use super::default_drum_rack_pads;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

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
