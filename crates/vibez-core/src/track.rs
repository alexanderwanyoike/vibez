use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::clip_timeline::FrameClipTimeline;
use crate::effect::EffectInfo;
use crate::id::{ClipId, TrackId};
use crate::midi::{InstrumentKind, TrackKind};
use crate::perform::SwingOffset;
use crate::transient::TransientMarkers;
use crate::warp_marker::WarpMarkers;

mod fades;
pub use fades::{ClipFades, FadeCurve};

/// Persisted hardware-input channel selection for an Audio Project Track.
/// Channel indexes are zero-based internally and presented as one-based labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum AudioInputRoute {
    Mono {
        channel: u16,
    },
    Stereo {
        left: u16,
    },
    /// Live post-device/post-fader output of one MIDI/instrument Project Track.
    Resample {
        track_id: TrackId,
    },
}

impl Default for AudioInputRoute {
    fn default() -> Self {
        Self::Mono { channel: 0 }
    }
}

impl std::fmt::Display for AudioInputRoute {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mono { channel } => write!(formatter, "IN {}", channel + 1),
            Self::Stereo { left } => write!(formatter, "IN {}/{}", left + 1, left + 2),
            Self::Resample { .. } => formatter.write_str("RESAMPLE"),
        }
    }
}

impl AudioInputRoute {
    pub fn resample_source(self) -> Option<TrackId> {
        match self {
            Self::Resample { track_id } => Some(track_id),
            Self::Mono { .. } | Self::Stereo { .. } => None,
        }
    }

    pub fn is_hardware(self) -> bool {
        self.resample_source().is_none()
    }
}

/// Explicit input-monitoring behaviour for an Audio Project Track.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputMonitoring {
    #[default]
    Off,
    Auto,
    On,
}

/// Nondestructive gain applied by one Audio Clip before its Project Track.
///
/// Keeping the range in a value object prevents project files and editor
/// messages from creating gains the Inspector cannot represent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ClipGainDb(f32);

impl ClipGainDb {
    pub const MIN: f32 = -70.0;
    pub const MAX: f32 = 24.0;

    pub fn new(db: f32) -> Option<Self> {
        db.is_finite().then(|| Self(db.clamp(Self::MIN, Self::MAX)))
    }

    pub const fn db(self) -> f32 {
        self.0
    }

    pub fn linear(self) -> f32 {
        10.0_f32.powf(self.0 / 20.0)
    }

    pub fn is_neutral(value: &Self) -> bool {
        value.0 == 0.0
    }
}

impl<'de> Deserialize<'de> for ClipGainDb {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let db = f32::deserialize(deserializer)?;
        Self::new(db).ok_or_else(|| serde::de::Error::custom("clip gain must be finite"))
    }
}

/// Nondestructive, duration-preserving Audio Clip pitch offset.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ClipTranspose(i8);

impl ClipTranspose {
    pub const MIN: i8 = -48;
    pub const MAX: i8 = 48;

    pub fn new(semitones: i8) -> Self {
        Self(semitones.clamp(Self::MIN, Self::MAX))
    }

    pub const fn semitones(self) -> i8 {
        self.0
    }

    pub const fn is_neutral(value: &Self) -> bool {
        value.0 == 0
    }
}

impl<'de> Deserialize<'de> for ClipTranspose {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let semitones = i8::deserialize(deserializer)?;
        Ok(Self::new(semitones))
    }
}

impl InputMonitoring {
    pub const ALL: [Self; 3] = [Self::Off, Self::Auto, Self::On];
}

impl std::fmt::Display for InputMonitoring {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Off => "OFF",
            Self::Auto => "AUTO",
            Self::On => "ON",
        })
    }
}

/// Credential-free identity retained after external media becomes project-owned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MediaProvenance {
    Local {
        source_path: PathBuf,
    },
    Remote {
        provider: String,
        connection_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        connection_name: Option<String>,
        source_id: String,
        source_path: String,
        revision: Option<String>,
    },
}

impl MediaProvenance {
    pub fn display_label(&self) -> String {
        match self {
            Self::Local { source_path } => source_path.display().to_string(),
            Self::Remote {
                connection_id,
                connection_name,
                source_path,
                ..
            } => format!(
                "{} · {source_path}",
                connection_name.as_deref().unwrap_or(connection_id)
            ),
        }
    }
}

/// Canonical reference to media outside the project file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MediaSourceRef {
    LocalFile {
        path: PathBuf,
    },
    /// Project-owned bytes copied to a staging area before the first save.
    /// This reference must never survive in a committed project document.
    StagedProjectMedia {
        id: String,
        file_name: String,
        staging_path: PathBuf,
        source_path: PathBuf,
    },
    /// Remote bytes copied out of disposable Media Cache into managed staging.
    /// Provider identity is descriptive provenance, never a playback path.
    StagedRemoteProjectMedia {
        id: String,
        file_name: String,
        staging_path: PathBuf,
        provenance: Box<MediaProvenance>,
    },
    /// Playback-critical media embedded in a Project Format V1 container.
    ProjectMedia {
        id: String,
        file_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provenance: Option<Box<MediaProvenance>>,
    },
    DropboxFile {
        path_lower: String,
        display_path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rev: Option<String>,
    },
}

impl MediaSourceRef {
    pub fn display_name(&self) -> String {
        match self {
            MediaSourceRef::LocalFile { path } => path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string()),
            MediaSourceRef::StagedProjectMedia { file_name, .. }
            | MediaSourceRef::StagedRemoteProjectMedia { file_name, .. }
            | MediaSourceRef::ProjectMedia { file_name, .. } => file_name.clone(),
            MediaSourceRef::DropboxFile { display_path, .. } => PathBuf::from(display_path)
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| display_path.clone()),
        }
    }

    pub fn provenance(&self) -> Option<&MediaProvenance> {
        match self {
            Self::StagedProjectMedia { .. } => None,
            Self::StagedRemoteProjectMedia { provenance, .. } => Some(provenance.as_ref()),
            Self::ProjectMedia { provenance, .. } => provenance.as_deref(),
            Self::LocalFile { .. } | Self::DropboxFile { .. } => None,
        }
    }
}

/// Persisted state for a future native drum rack.
pub const DRUM_RACK_PAD_COUNT: usize = 16;
pub const DRUM_RACK_BASE_NOTE: u8 = 36;

pub const fn drum_rack_pad_pitch(pad_index: usize) -> Option<u8> {
    if pad_index < DRUM_RACK_PAD_COUNT {
        Some(DRUM_RACK_BASE_NOTE + pad_index as u8)
    } else {
        None
    }
}

pub const fn drum_rack_pad_index(pitch: u8) -> Option<usize> {
    if pitch >= DRUM_RACK_BASE_NOTE && pitch < DRUM_RACK_BASE_NOTE + DRUM_RACK_PAD_COUNT as u8 {
        Some((pitch - DRUM_RACK_BASE_NOTE) as usize)
    } else {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrumPadState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub source: Option<MediaSourceRef>,
    pub gain: f32,
    pub pan: f32,
    pub start: f32,
    pub end: f32,
    pub coarse_tune: i8,
    pub fine_tune: f32,
    pub one_shot: bool,
    pub choke_group: Option<u8>,
}

/// Persisted state for native instruments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InstrumentStateInfo {
    SubtractiveSynth {
        params: Vec<f32>,
    },
    Sampler {
        params: Vec<f32>,
        source: Option<MediaSourceRef>,
    },
    DrumRack {
        pads: Vec<DrumPadState>,
    },
}

/// Serializable track metadata shared between engine and UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackInfo {
    pub id: TrackId,
    pub name: String,
    pub gain: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    /// Hardware Audio Input routing. Meaningful only for Audio Tracks.
    #[serde(default)]
    pub audio_input_route: AudioInputRoute,
    /// Hardware input monitoring. Track arm itself remains runtime state.
    #[serde(default)]
    pub input_monitoring: InputMonitoring,
    /// Optional adjustment combined with the Project Swing amount for
    /// generated events on this Project Track.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swing_offset: Option<SwingOffset>,
    #[serde(default)]
    pub effects: Vec<EffectInfo>,
    #[serde(default)]
    pub kind: TrackKind,
    #[serde(default)]
    pub color_index: u8,
    #[serde(default)]
    pub instrument: Option<InstrumentKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_instrument: Option<InstrumentStateInfo>,
    /// Third-party plugin instrument on this track, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_instrument: Option<crate::effect::PluginDeviceInfo>,
    /// Automation lanes on this track.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub automation: Vec<crate::automation::AutomationLane>,
    /// Post-fader send amounts into buses: `(bus id, 0..1)`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sends: Vec<(TrackId, f32)>,
}

impl TrackInfo {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: TrackId::new(),
            name: name.into(),
            gain: crate::constants::DEFAULT_TRACK_GAIN,
            pan: crate::constants::DEFAULT_TRACK_PAN,
            mute: false,
            solo: false,
            audio_input_route: AudioInputRoute::default(),
            input_monitoring: InputMonitoring::default(),
            swing_offset: None,
            effects: Vec::new(),
            kind: TrackKind::default(),
            color_index: 0,
            instrument: None,
            native_instrument: None,
            plugin_instrument: None,
            automation: Vec::new(),
            sends: Vec::new(),
        }
    }
}

/// Direction in which an Audio Clip traverses its resolved visible playback.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipPlaybackDirection {
    #[default]
    Forward,
    Reverse,
}

impl ClipPlaybackDirection {
    pub const fn toggled(self) -> Self {
        match self {
            Self::Forward => Self::Reverse,
            Self::Reverse => Self::Forward,
        }
    }

    pub const fn map_clip_frame(self, clip_frame: u64, duration: u64) -> u64 {
        match self {
            Self::Forward => clip_frame,
            Self::Reverse => duration.saturating_sub(1).saturating_sub(clip_frame),
        }
    }

    pub const fn is_forward(value: &Self) -> bool {
        matches!(value, Self::Forward)
    }
}

/// Serializable clip metadata shared between engine and UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipInfo {
    pub id: ClipId,
    pub track_id: TrackId,
    pub name: String,
    /// Position on the timeline in samples.
    pub position: u64,
    /// Offset into the source audio in samples.
    pub source_offset: u64,
    /// Initial playback position inside the source window. Older projects
    /// omit it and inherit `source_offset` when loaded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_marker: Option<u64>,
    /// Duration in samples.
    pub duration: u64,
    /// Canonical external media source reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<MediaSourceRef>,
    /// Legacy local path kept for backward compatibility with older projects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<PathBuf>,
    #[serde(default)]
    pub loop_enabled: bool,
    #[serde(default)]
    pub loop_start: u64,
    #[serde(default)]
    pub loop_end: u64,
    /// Nondestructive per-Clip gain before the Project Track channel strip.
    #[serde(default, skip_serializing_if = "ClipGainDb::is_neutral")]
    pub gain_db: ClipGainDb,
    /// Fade lengths at the visible Clip edges.
    #[serde(default, skip_serializing_if = "ClipFades::is_neutral")]
    pub fades: ClipFades,
    /// Nondestructive traversal direction over the resolved Clip playback.
    #[serde(default, skip_serializing_if = "ClipPlaybackDirection::is_forward")]
    pub playback_direction: ClipPlaybackDirection,
    /// Source-frame Transient Markers used by later Warp and Slice operations.
    #[serde(default, skip_serializing_if = "TransientMarkers::is_neutral")]
    pub transient_markers: TransientMarkers,
    /// Piecewise source-to-Clip timing map. Empty means identity timing.
    #[serde(default, skip_serializing_if = "WarpMarkers::is_neutral")]
    pub warp_markers: WarpMarkers,
    /// Duration-preserving pitch offset in semitones.
    #[serde(default, skip_serializing_if = "ClipTranspose::is_neutral")]
    pub transpose: ClipTranspose,
    /// Nominal BPM of the underlying sample, set either by BPM
    /// detection or manually. Drives warp ratio calculations and is
    /// independent of the project tempo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_bpm: Option<f64>,
    /// Whether the clip's audio has been time-stretched to fit the
    /// project tempo.
    #[serde(default, skip_serializing_if = "skip_if_false")]
    pub warped: bool,
    /// Project BPM the current warped audio was stretched to. Used to
    /// flag staleness when the project tempo changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warped_to_bpm: Option<f64>,
}

fn skip_if_false(b: &bool) -> bool {
    !b
}

impl ClipInfo {
    /// The end position of this clip on the timeline (position + duration).
    pub fn end_position(&self) -> u64 {
        self.position.saturating_add(self.duration)
    }

    /// Resolve the persisted Start marker into the playable source window.
    /// Legacy projects begin at Source In, matching pre-marker playback.
    pub fn resolved_start_marker(&self, source_frames: u64) -> u64 {
        let source_end = self
            .source_offset
            .saturating_add(self.duration)
            .min(source_frames);
        FrameClipTimeline::new(
            self.start_marker.unwrap_or(self.source_offset),
            self.loop_start,
            self.loop_end,
            self.duration,
            self.loop_enabled,
        )
        .clamp_start(
            self.start_marker.unwrap_or(self.source_offset),
            self.source_offset,
            source_end,
        )
    }

    pub fn resolved_source(&self) -> Option<&MediaSourceRef> {
        self.source.as_ref()
    }

    pub fn resolved_local_path(&self) -> Option<&PathBuf> {
        if let Some(MediaSourceRef::LocalFile { path }) = self.source.as_ref() {
            Some(path)
        } else {
            self.file_path.as_ref()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_property_value_objects_enforce_the_inspector_ranges() {
        assert_eq!(ClipGainDb::new(30.0).unwrap().db(), ClipGainDb::MAX);
        assert_eq!(ClipGainDb::new(-80.0).unwrap().db(), ClipGainDb::MIN);
        assert!(ClipGainDb::new(f32::NAN).is_none());
        assert_eq!(ClipTranspose::new(60).semitones(), ClipTranspose::MAX);
        assert_eq!(ClipTranspose::new(-60).semitones(), ClipTranspose::MIN);
    }

    #[test]
    fn track_info_defaults() {
        let track = TrackInfo::new("Track 1");
        assert_eq!(track.name, "Track 1");
        assert!((track.gain - 1.0).abs() < f32::EPSILON);
        assert!((track.pan - 0.5).abs() < f32::EPSILON);
        assert!(!track.mute);
        assert!(!track.solo);
        assert_eq!(
            track.audio_input_route,
            AudioInputRoute::Mono { channel: 0 }
        );
        assert_eq!(track.input_monitoring, InputMonitoring::Off);
    }

    #[test]
    fn audio_input_routing_roundtrips_and_old_tracks_receive_safe_defaults() {
        let mut track = TrackInfo::new("Vocal");
        track.audio_input_route = AudioInputRoute::Stereo { left: 2 };
        track.input_monitoring = InputMonitoring::Auto;
        let encoded = serde_json::to_string(&track).unwrap();
        let decoded: TrackInfo = serde_json::from_str(&encoded).unwrap();
        assert_eq!(
            decoded.audio_input_route,
            AudioInputRoute::Stereo { left: 2 }
        );
        assert_eq!(decoded.input_monitoring, InputMonitoring::Auto);

        let mut legacy = serde_json::to_value(track).unwrap();
        legacy.as_object_mut().unwrap().remove("audio_input_route");
        legacy.as_object_mut().unwrap().remove("input_monitoring");
        let decoded: TrackInfo = serde_json::from_value(legacy).unwrap();
        assert_eq!(decoded.audio_input_route, AudioInputRoute::default());
        assert_eq!(decoded.input_monitoring, InputMonitoring::Off);

        let source = TrackId::new();
        let mut track = TrackInfo::new("Resample");
        track.audio_input_route = AudioInputRoute::Resample { track_id: source };
        let decoded: TrackInfo =
            serde_json::from_str(&serde_json::to_string(&track).unwrap()).unwrap();
        assert_eq!(
            decoded.audio_input_route,
            AudioInputRoute::Resample { track_id: source }
        );
    }

    fn test_clip(position: u64, duration: u64) -> ClipInfo {
        ClipInfo {
            id: ClipId::new(),
            track_id: TrackId::new(),
            name: "test".into(),
            position,
            source_offset: 0,
            start_marker: None,
            duration,
            source: Some(MediaSourceRef::LocalFile {
                path: PathBuf::from("test.wav"),
            }),
            file_path: Some(PathBuf::from("test.wav")),
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            gain_db: Default::default(),
            fades: Default::default(),
            playback_direction: Default::default(),
            transient_markers: Default::default(),
            warp_markers: Default::default(),
            transpose: Default::default(),
            original_bpm: None,
            warped: false,
            warped_to_bpm: None,
        }
    }

    #[test]
    fn clip_end_position() {
        let clip = test_clip(1000, 500);
        assert_eq!(clip.end_position(), 1500);
    }

    #[test]
    fn clip_end_position_saturates() {
        let clip = test_clip(u64::MAX - 10, 100);
        assert_eq!(clip.end_position(), u64::MAX);
    }

    #[test]
    fn unique_ids() {
        let t1 = TrackInfo::new("A");
        let t2 = TrackInfo::new("B");
        assert_ne!(t1.id, t2.id);
    }

    #[test]
    fn serde_roundtrip_track() {
        let track = TrackInfo::new("Synth Lead");
        let json = serde_json::to_string(&track).unwrap();
        let deserialized: TrackInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(track.id, deserialized.id);
        assert_eq!(track.name, deserialized.name);
        assert!((track.gain - deserialized.gain).abs() < f32::EPSILON);
    }

    #[test]
    fn serde_roundtrip_clip() {
        let mut clip = test_clip(44_100, 88_200);
        clip.name = "vocal.wav".into();
        clip.source_offset = 1_000;
        clip.start_marker = Some(2_000);
        clip.source = Some(MediaSourceRef::LocalFile {
            path: PathBuf::from("/audio/vocal.wav"),
        });
        clip.file_path = Some(PathBuf::from("/audio/vocal.wav"));
        clip.original_bpm = Some(174.0);
        clip.gain_db = ClipGainDb::new(-3.5).unwrap();
        clip.fades = ClipFades::new(4_410, 8_820, clip.duration).linked_fade_out(
            8_820,
            ClipId::new(),
            clip.duration,
        );
        clip.playback_direction = ClipPlaybackDirection::Reverse;
        clip.transient_markers.add_authored(12_345);
        clip.transient_markers.replace_suggestions([4_410, 22_050]);
        clip.warp_markers
            .add(22_050, 30_000, clip.source_offset, 89_200, clip.duration);
        clip.transpose = ClipTranspose::new(7);
        clip.warped = true;
        clip.warped_to_bpm = Some(140.0);
        let json = serde_json::to_string(&clip).unwrap();
        let deserialized: ClipInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(clip.id, deserialized.id);
        assert_eq!(clip.position, deserialized.position);
        assert_eq!(clip.source_offset, deserialized.source_offset);
        assert_eq!(clip.start_marker, deserialized.start_marker);
        assert_eq!(clip.duration, deserialized.duration);
        assert_eq!(clip.file_path, deserialized.file_path);
        assert_eq!(clip.source, deserialized.source);
        assert_eq!(clip.original_bpm, deserialized.original_bpm);
        assert_eq!(clip.gain_db, deserialized.gain_db);
        assert_eq!(clip.fades, deserialized.fades);
        assert_eq!(clip.playback_direction, deserialized.playback_direction);
        assert_eq!(clip.transient_markers, deserialized.transient_markers);
        assert_eq!(clip.warp_markers, deserialized.warp_markers);
        assert_eq!(clip.transpose, deserialized.transpose);
        assert_eq!(clip.warped, deserialized.warped);
        assert_eq!(clip.warped_to_bpm, deserialized.warped_to_bpm);
    }

    #[test]
    fn resolved_start_marker_defaults_and_clamps_to_the_playable_window() {
        let mut clip = test_clip(0, 100);
        clip.source_offset = 10;
        clip.loop_enabled = true;
        clip.loop_end = 80;
        assert_eq!(clip.resolved_start_marker(200), 10);

        clip.start_marker = Some(120);
        assert_eq!(clip.resolved_start_marker(200), 79);
    }

    #[test]
    fn clip_info_backward_compat_from_legacy_file_path() {
        let json = r#"{
            "id":0,
            "track_id":0,
            "name":"legacy.wav",
            "position":0,
            "source_offset":0,
            "duration":100,
            "file_path":"legacy.wav"
        }"#;
        let clip: ClipInfo = serde_json::from_str(json).unwrap();
        assert_eq!(clip.file_path, Some(PathBuf::from("legacy.wav")));
        assert!(clip.source.is_none());
        assert_eq!(clip.gain_db, ClipGainDb::default());
        assert_eq!(clip.fades, ClipFades::default());
        assert_eq!(clip.playback_direction, ClipPlaybackDirection::Forward);
        assert!(clip.transient_markers.is_empty());
        assert!(clip.warp_markers.is_empty());
        assert_eq!(clip.transpose, ClipTranspose::default());
        assert_eq!(clip.start_marker, None);
    }

    #[test]
    fn clip_fades_enforce_duration_and_render_the_visible_edges() {
        let fades = ClipFades::new(4, 4, 8);
        assert_eq!(fades.gain_at(0, 8), 0.0);
        assert_eq!(fades.gain_at(2, 8), 0.5);
        assert_eq!(fades.gain_at(3, 8), 0.75);
        assert_eq!(fades.gain_at(4, 8), 0.75);
        assert_eq!(fades.gain_at(7, 8), 0.0);

        let clamped = ClipFades::new(7, 7, 10);
        assert_eq!(clamped.fade_in_frames(), 7);
        assert_eq!(clamped.fade_out_frames(), 3);
    }

    #[test]
    fn fade_curves_are_bounded_reciprocal_shapes_with_linear_legacy_defaults() {
        let late = FadeCurve::new(-100);
        let linear = FadeCurve::new(0);
        let early = FadeCurve::new(100);

        assert_eq!(late.percent(), -100);
        assert_eq!(FadeCurve::new(-500).percent(), -100);
        assert_eq!(FadeCurve::new(500).percent(), 100);
        assert!((late.gain(0.5) - 0.0625).abs() < 1e-6);
        assert!((linear.gain(0.5) - 0.5).abs() < 1e-6);
        assert!((early.gain(0.5) - 0.840_896_4).abs() < 1e-6);

        let legacy: ClipFades =
            serde_json::from_str(r#"{"fade_in_frames":4,"fade_out_frames":2}"#).unwrap();
        assert_eq!(legacy.fade_in_curve(), FadeCurve::default());
        assert_eq!(legacy.fade_out_curve(), FadeCurve::default());

        let shaped = ClipFades::new(4, 2, 8)
            .with_fade_in_curve(early)
            .with_fade_out_curve(late);
        let reopened: ClipFades =
            serde_json::from_str(&serde_json::to_string(&shaped).unwrap()).unwrap();
        assert_eq!(reopened, shaped);
    }

    #[test]
    fn persisted_fades_clamp_wide_curve_values_and_normalize_empty_edges() {
        let curve: FadeCurve = serde_json::from_str("500").unwrap();
        assert_eq!(curve.percent(), FadeCurve::MAX);

        let fades: ClipFades = serde_json::from_str(
            r#"{"fade_in_frames":0,"fade_out_frames":0,"fade_in_curve":50,"fade_out_curve":-50}"#,
        )
        .unwrap();
        assert_eq!(fades.fade_in_curve(), FadeCurve::default());
        assert_eq!(fades.fade_out_curve(), FadeCurve::default());
        assert!(ClipFades::is_neutral(&fades));
    }

    #[test]
    fn independent_fade_curves_change_audio_and_survive_length_edits() {
        let fades = ClipFades::new(4, 4, 8)
            .with_fade_in_curve(FadeCurve::new(100))
            .with_fade_out_curve(FadeCurve::new(-100));

        assert!((fades.gain_at(2, 8) - 0.840_896_4).abs() < 1e-6);
        assert!((fades.gain_at(5, 8) - 0.0625).abs() < 1e-6);
        assert_eq!(fades.with_fade_in(3, 8).fade_in_curve().percent(), 100);
        assert_eq!(fades.with_fade_out(0, 8).fade_out_curve().percent(), 0);
    }

    #[test]
    fn neutral_clip_fades_do_not_change_audio_gain() {
        let fades = ClipFades::default();
        for frame in 0..8 {
            assert_eq!(fades.gain_at(frame, 8), 1.0);
        }
    }

    #[test]
    fn clip_fragments_keep_only_the_fades_at_edges_they_contain() {
        let fades = ClipFades::new(10, 20, 100);
        assert_eq!(fades.for_fragment(100, 0, 40), ClipFades::new(10, 0, 40));
        assert_eq!(fades.for_fragment(100, 40, 60), ClipFades::new(0, 20, 60));
        assert_eq!(fades.for_fragment(100, 20, 40), ClipFades::default());
    }

    #[test]
    fn linked_crossfade_edges_form_an_equal_power_pair() {
        let outgoing_id = ClipId::new();
        let incoming_id = ClipId::new();
        let frames = 16;
        let outgoing = ClipFades::default().linked_fade_out(frames, incoming_id, frames);
        let incoming = ClipFades::default().linked_fade_in(frames, outgoing_id, frames);

        for frame in 0..frames {
            let power =
                outgoing.gain_at(frame, frames).powi(2) + incoming.gain_at(frame, frames).powi(2);
            assert!((power - 1.0).abs() < 1e-5, "frame {frame}: {power}");
        }
    }
}
