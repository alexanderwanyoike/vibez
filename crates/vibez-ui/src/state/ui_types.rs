//! UI-side data types for tracks, clips, devices, and grids.

use super::default_drum_rack_pads;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::clip_timeline::{BeatClipTimeline, FrameClipTimeline};
use vibez_core::effect::{EffectType, ParamDescriptor};
use vibez_core::id::{ClipId, EffectId, TrackId};
use vibez_core::midi::{InstrumentKind, MidiNote, TrackKind, NOTE_CONTENT_END_BEATS};
use vibez_core::track::{
    AudioInputRoute, ClipFades, ClipGainDb, ClipPlaybackDirection, ClipTranspose, DrumPadState,
    InputMonitoring, MediaSourceRef,
};
use vibez_core::transient::TransientMarkers;
use vibez_core::warp_marker::WarpMarkers;

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
    /// Initial playback position. This may differ from `loop_start`, allowing
    /// an intro or pickup to play once before the loop repeats.
    pub start_marker: u64,
    /// Duration in samples.
    pub duration: u64,
    // Looping
    pub loop_enabled: bool,
    pub loop_start: u64,
    pub loop_end: u64,
    pub gain_db: ClipGainDb,
    pub fades: ClipFades,
    pub playback_direction: ClipPlaybackDirection,
    pub transient_markers: TransientMarkers,
    pub warp_markers: WarpMarkers,
    pub transpose: ClipTranspose,
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

/// Decoded audio compares by buffer identity: comparing samples would scan
/// whole recordings, and every edit that changes audio installs a new buffer.
impl PartialEq for UiClip {
    fn eq(&self, other: &Self) -> bool {
        let Self {
            id,
            name,
            audio,
            source,
            position,
            source_offset,
            start_marker,
            duration,
            loop_enabled,
            loop_start,
            loop_end,
            gain_db,
            fades,
            playback_direction,
            transient_markers,
            warp_markers,
            transpose,
            original_bpm,
            warped,
            warped_to_bpm,
            original_audio,
        } = self;
        *id == other.id
            && Arc::ptr_eq(audio, &other.audio)
            && *position == other.position
            && *source_offset == other.source_offset
            && *start_marker == other.start_marker
            && *duration == other.duration
            && *loop_enabled == other.loop_enabled
            && *loop_start == other.loop_start
            && *loop_end == other.loop_end
            && *gain_db == other.gain_db
            && *fades == other.fades
            && *playback_direction == other.playback_direction
            && *warp_markers == other.warp_markers
            && *name == other.name
            && *source == other.source
            && *transient_markers == other.transient_markers
            && *transpose == other.transpose
            && *original_bpm == other.original_bpm
            && *warped == other.warped
            && *warped_to_bpm == other.warped_to_bpm
            && same_audio(original_audio, &other.original_audio)
    }
}

pub(super) fn same_audio(
    left: &Option<Arc<DecodedAudio>>,
    right: &Option<Arc<DecodedAudio>>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => Arc::ptr_eq(left, right),
        (left, right) => left.is_none() && right.is_none(),
    }
}

/// Where a Clip sits on its lane, in beats.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipSpan {
    pub start_beats: f64,
    pub length_beats: f64,
}

impl ClipSpan {
    pub fn end_beats(self) -> f64 {
        self.start_beats + self.length_beats
    }
}

impl UiClip {
    pub fn span(&self, samples_per_beat: f64) -> ClipSpan {
        ClipSpan {
            start_beats: self.position as f64 / samples_per_beat,
            length_beats: self.duration as f64 / samples_per_beat,
        }
    }

    pub(crate) fn source_end(&self) -> u64 {
        self.warp_markers.source_end(
            self.source_offset
                .saturating_add(self.duration)
                .min(self.audio.num_frames() as u64),
        )
    }

    /// The length `requested` may actually take, as an unlooped Clip stops
    /// where its sample runs out.
    pub(crate) fn playable_duration(&self, requested: u64) -> u64 {
        if self.loop_enabled {
            return requested;
        }
        let frames = self.audio.num_frames() as u64;
        let longest = (self.warp_timeline_end() + frames.saturating_sub(self.source_end()))
            .saturating_sub(self.start_marker.saturating_sub(self.source_offset));
        requested.min(longest)
    }

    /// Change the length to `requested`, or as near as the sample allows,
    /// moving the right edge.
    pub(crate) fn resize(&mut self, requested: u64) {
        let new_duration = self.playable_duration(requested);
        let old_mapping_end = self.warp_timeline_end();
        let new_mapping_end =
            new_duration.min((self.audio.num_frames() as u64).saturating_sub(self.source_offset));
        self.warp_markers = self.warp_markers.resized_timeline(
            old_mapping_end,
            new_mapping_end,
            self.source_offset,
            self.audio.num_frames() as u64,
        );
        self.duration = new_duration;
        self.clamp_fades_to_clip();
        self.clamp_start_to_source();
        if self.loop_enabled {
            self.clamp_loop_to_warp_map();
        }
        self.transient_markers
            .retain_source_range(self.source_offset, self.source_end());
    }

    pub(crate) fn warp_timeline_end(&self) -> u64 {
        let identity_end = self
            .duration
            .min((self.audio.num_frames() as u64).saturating_sub(self.source_offset));
        self.warp_markers.timeline_end(identity_end)
    }

    pub(crate) fn timeline(&self) -> FrameClipTimeline {
        FrameClipTimeline::new(
            self.start_marker,
            self.loop_start,
            self.loop_end,
            self.duration,
            self.loop_enabled,
        )
    }

    /// Only a loop lets the start marker sit past the clip end, so the
    /// marker returns to the clip start with the region it was bounded by.
    pub(crate) fn reset_loop_region_to_clip(&mut self) {
        let end = self.source_offset.saturating_add(self.duration);
        if self.start_marker >= end {
            self.start_marker = self.source_offset;
        }
        self.loop_start = self.start_marker;
        self.loop_end = end;
    }

    /// The loop region outlives its loop being off, as Ableton keeps a clip's
    /// loop brace, so turning it back on loops the same region.
    pub(crate) fn set_loop_enabled(&mut self, enabled: bool) {
        self.loop_enabled = enabled;
        if enabled && !self.accepts_loop_region(self.loop_start, self.loop_end) {
            self.reset_loop_region_to_clip();
        }
    }

    pub(crate) fn loop_content_start(&self) -> u64 {
        FrameClipTimeline::loop_content_start(self.source_offset, &self.warp_markers)
    }

    pub(crate) fn loop_content_end(&self) -> u64 {
        FrameClipTimeline::loop_content_end(
            self.source_offset,
            self.audio.num_frames() as u64,
            &self.warp_markers,
        )
    }

    /// The frames the audio editor spans: from where the loop may begin to
    /// the furthest it may reach, or to the clip's end where an unlooped
    /// clip runs on into silence.
    pub(crate) fn editor_frames(&self) -> (u64, u64) {
        let reach = self.loop_content_end();
        let end = if self.loop_enabled {
            reach
        } else {
            reach.max(self.source_offset.saturating_add(self.duration))
        };
        (self.loop_content_start(), end)
    }

    /// Resizing a warped clip resizes its Warp Marker map, which can cut the
    /// loop.
    pub(crate) fn clamp_loop_to_warp_map(&mut self) {
        if self.warp_markers.is_empty() {
            return;
        }
        self.loop_end = self.loop_end.min(self.loop_content_end());
        if self.loop_end <= self.loop_start || self.start_marker >= self.loop_end {
            self.reset_loop_region_to_clip();
        }
    }

    pub(crate) fn clamp_start_to_source(&mut self) {
        let source_end = self
            .source_offset
            .saturating_add(self.duration)
            .min(self.audio.num_frames() as u64);
        self.start_marker =
            self.timeline()
                .clamp_start(self.start_marker, self.source_offset, source_end);
    }

    pub(crate) fn set_start_marker(&mut self, start_marker: u64) -> bool {
        let source_end = self
            .source_offset
            .saturating_add(self.duration)
            .min(self.audio.num_frames() as u64);
        let valid = self
            .timeline()
            .accepts_start(start_marker, self.source_offset, source_end);
        if !valid || start_marker == self.start_marker {
            return false;
        }
        self.start_marker = start_marker;
        true
    }

    fn accepts_loop_region(&self, start: u64, end: u64) -> bool {
        start >= self.loop_content_start()
            && start < end
            && end <= self.loop_content_end()
            && self.start_marker < end
    }

    pub(crate) fn set_loop_region(&mut self, start: u64, end: u64) -> bool {
        if !self.accepts_loop_region(start, end) || (start, end) == (self.loop_start, self.loop_end)
        {
            return false;
        }
        self.loop_start = start;
        self.loop_end = end;
        true
    }

    /// A loop region may reach past the clip end, so new source bounds only
    /// pull its start up to the source start; a region that still does not
    /// fit becomes the clip's.
    pub(crate) fn fit_loop_region_to_source(&mut self) {
        let start = self.loop_start.max(self.source_offset);
        if self.accepts_loop_region(start, self.loop_end) {
            self.loop_start = start;
        } else {
            self.reset_loop_region_to_clip();
        }
    }

    pub(crate) fn clamp_fades_to_clip(&mut self) {
        self.fades = self.fades.clamped_to(self.duration);
    }

    pub(crate) fn source_frame_at(&self, clip_frame: u64) -> u64 {
        self.source_frame_position_at(clip_frame).round() as u64
    }

    pub(crate) fn source_frame_position_at(&self, clip_frame: u64) -> f64 {
        let timeline_frame = self.timeline().source_at(
            self.playback_direction
                .map_clip_frame(clip_frame, self.duration),
        );
        if self.warp_markers.is_empty() {
            return timeline_frame as f64;
        }
        self.warp_markers.source_at_timeline(
            timeline_frame.saturating_sub(self.source_offset) as f64,
            self.source_offset,
            self.warp_timeline_end(),
        )
    }

    pub(crate) fn has_same_audible_geometry(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.audio, &other.audio)
            && self.duration == other.duration
            && self.source_offset == other.source_offset
            && self.start_marker == other.start_marker
            && self.loop_enabled == other.loop_enabled
            && self.loop_start == other.loop_start
            && self.loop_end == other.loop_end
            && self.gain_db == other.gain_db
            && self.fades == other.fades
            && self.playback_direction == other.playback_direction
            && self.warp_markers == other.warp_markers
            && self.transpose == other.transpose
            && self.warped == other.warped
            && self.warped_to_bpm == other.warped_to_bpm
    }

    pub(crate) fn timeline_frame_at_source(&self, source_frame: u64) -> u64 {
        self.warp_markers
            .timeline_at_source(source_frame as f64, self.source_offset, self.source_end())
            .round() as u64
    }

    pub(crate) fn warp_geometry_for_fragment(
        &self,
        local_start: u64,
        duration: u64,
    ) -> (u64, u64, WarpMarkers) {
        let phase = match self.playback_direction {
            ClipPlaybackDirection::Forward => local_start,
            ClipPlaybackDirection::Reverse => self
                .duration
                .saturating_sub(local_start)
                .saturating_sub(duration),
        };
        let timeline_start = self.timeline().source_at(phase);
        if self.warp_markers.is_empty() {
            return (timeline_start, timeline_start, WarpMarkers::default());
        }
        if self.timeline().is_looping() {
            return (
                self.source_offset,
                timeline_start,
                self.warp_markers.clone(),
            );
        }
        let (source_offset, markers) = self.warp_markers.for_fragment(
            timeline_start.saturating_sub(self.source_offset),
            duration,
            self.source_offset,
            self.warp_timeline_end(),
        );
        (source_offset, source_offset, markers)
    }
}

/// Editable numeric fields in Audio Clip Inspector V1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AudioClipInspectorField {
    Gain,
    FadeIn,
    FadeOut,
    SourceBpm,
    SourceStart,
    SourceEnd,
    Start,
    LoopStart,
    LoopEnd,
    Transpose,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioClipFadeEdge {
    In,
    Out,
}

/// Inspector fields that can be controlled by a rotary widget.
///
/// Keeping this narrower than [`AudioClipInspectorField`] makes it impossible
/// for a knob to emit a source or loop-bound edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioClipRotaryField {
    Gain,
    Transpose,
}

impl AudioClipRotaryField {
    pub const fn inspector_field(self) -> AudioClipInspectorField {
        match self {
            Self::Gain => AudioClipInspectorField::Gain,
            Self::Transpose => AudioClipInspectorField::Transpose,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UiDrumPad {
    pub name: Option<String>,
    pub source: Option<MediaSourceRef>,
    /// Decoded audio kept on the UI side so offline bounce can re-seed a
    /// drum rack without a round-trip through the audio thread.
    pub audio: Option<Arc<DecodedAudio>>,
    pub choke_group: Option<u8>,
    /// The pad's Sampler values, all of them.
    pub params: Vec<f32>,
}

impl Default for UiDrumPad {
    fn default() -> Self {
        Self::from_state(&DrumPadState::default())
    }
}

impl UiDrumPad {
    /// The pad's Sampler values, first worked out from the ones it was
    /// saved with when it has fewer than the Sampler now has.
    pub fn sampler_params_mut(&mut self) -> &mut Vec<f32> {
        if self.params.len() < vibez_instruments::sampler::param::COUNT {
            self.params = vibez_instruments::drum_rack::pad_params(&self.to_state()).to_vec();
        }
        &mut self.params
    }

    /// Whether the pad names a sample, even one that has not decoded.
    pub fn holds_sample(&self) -> bool {
        self.source.is_some() || self.audio.is_some()
    }

    pub fn param(&self, index: usize) -> f32 {
        self.params.get(index).copied().unwrap_or_else(|| {
            vibez_instruments::sampler::SAMPLER_PARAMS
                .get(index)
                .map_or(0.0, |descriptor| descriptor.default)
        })
    }

    pub fn to_state(&self) -> DrumPadState {
        DrumPadState {
            name: self.name.clone(),
            source: self.source.clone(),
            choke_group: self.choke_group,
            params: self.params.clone(),
            ..DrumPadState::default()
        }
    }

    /// A pad as saved, its older settings translated to Sampler values.
    pub fn from_state(state: &DrumPadState) -> Self {
        Self {
            name: state
                .name
                .clone()
                .or_else(|| state.source.as_ref().map(MediaSourceRef::display_name)),
            source: state.source.clone(),
            audio: None,
            choke_group: state.choke_group,
            params: vibez_instruments::drum_rack::pad_params(state).to_vec(),
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
    /// A live owner whose main-thread restart failed remains a guarded node.
    pub reconfiguration_failed: bool,
    pub latency_samples: Option<u32>,
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
    pub sidechains: Vec<vibez_core::routing::SidechainAssignment>,
    pub inactive_sidechains: Vec<vibez_core::routing::ExternalInputId>,
    pub external_inputs: Vec<vibez_core::routing::ExternalInputDescriptor>,
}

impl UiEffect {
    pub fn unavailable_plugin(info: &vibez_core::effect::EffectInfo) -> Self {
        Self {
            reconfiguration_failed: false,
            latency_samples: None,
            id: info.id,
            effect_type: info.effect_type,
            bypass: info.bypass,
            params: info.params.clone(),
            descriptors: &[],
            plugin_name: info.plugin.as_ref().map(|plugin| plugin.name.clone()),
            has_plugin_gui: false,
            plugin_ref: info.plugin.clone(),
            inactive_sidechains: info.inactive_sidechains.clone(),
            sidechains: info.sidechains.clone(),
            external_inputs: Vec::new(),
        }
    }
}

/// A note clip (MIDI pattern) as represented in the UI.
#[derive(Debug, Clone, PartialEq)]
pub struct UiNoteClip {
    pub id: ClipId,
    pub name: String,
    pub position_beats: f64,
    pub duration_beats: f64,
    pub notes: Vec<MidiNote>,
    pub selected_notes: HashSet<usize>,
    pub start_marker_beats: f64,
    // Looping
    pub loop_enabled: bool,
    pub loop_start_beats: f64,
    pub loop_end_beats: f64,
    pub groove_grid: vibez_core::perform::GrooveGrid,
}

impl UiNoteClip {
    pub fn span(&self) -> ClipSpan {
        ClipSpan {
            start_beats: self.position_beats,
            length_beats: self.duration_beats,
        }
    }

    pub(crate) fn timeline(&self) -> BeatClipTimeline {
        BeatClipTimeline::new(
            self.start_marker_beats,
            self.loop_start_beats,
            self.loop_end_beats,
            self.duration_beats,
            self.loop_enabled,
        )
    }

    /// See `UiClip::reset_loop_region_to_clip`.
    pub(crate) fn reset_loop_region_to_clip(&mut self) {
        if self.start_marker_beats >= self.duration_beats {
            self.start_marker_beats = 0.0;
        }
        self.loop_start_beats = self.start_marker_beats;
        self.loop_end_beats = self.duration_beats;
    }

    /// See `UiClip::set_loop_enabled`.
    pub(crate) fn set_loop_enabled(&mut self, enabled: bool) {
        self.loop_enabled = enabled;
        if enabled && !self.accepts_loop_region(self.loop_start_beats, self.loop_end_beats) {
            self.reset_loop_region_to_clip();
        }
    }

    /// Change the length to `duration_beats`, moving the right edge.
    pub(crate) fn resize(&mut self, duration_beats: f64) {
        self.duration_beats = duration_beats;
        self.clamp_start_to_content();
    }

    pub(crate) fn clamp_start_to_content(&mut self) {
        self.start_marker_beats =
            self.timeline()
                .clamp_start(self.start_marker_beats, 0.0, NOTE_CONTENT_END_BEATS);
    }

    /// The content beat this Clip plays at Arrangement `beat`; off the
    /// Clip it plays nothing.
    pub(crate) fn content_beat_at(&self, beat: f64) -> Option<f64> {
        let into_clip = beat - self.position_beats;
        (0.0..self.duration_beats)
            .contains(&into_clip)
            .then(|| self.timeline().source_at(into_clip))
    }

    /// Content beats a clip editor shows, from beat 0 to where the Clip
    /// stops playing.
    pub(crate) fn editor_beats(&self) -> f64 {
        if self.timeline().is_looping() {
            self.duration_beats
        } else {
            self.start_marker_beats + self.duration_beats
        }
    }

    pub(crate) fn set_start_marker(&mut self, start_marker_beats: f64) -> bool {
        let valid = self
            .timeline()
            .accepts_start(start_marker_beats, 0.0, NOTE_CONTENT_END_BEATS);
        if !valid || start_marker_beats == self.start_marker_beats {
            return false;
        }
        self.start_marker_beats = start_marker_beats;
        true
    }

    fn accepts_loop_region(&self, start: f64, end: f64) -> bool {
        start.is_finite()
            && end.is_finite()
            && start >= 0.0
            && start < end
            && end <= NOTE_CONTENT_END_BEATS
            && self.start_marker_beats < end
    }

    pub(crate) fn set_loop_region(&mut self, start: f64, end: f64) -> bool {
        if !self.accepts_loop_region(start, end)
            || (start, end) == (self.loop_start_beats, self.loop_end_beats)
        {
            return false;
        }
        self.loop_start_beats = start;
        self.loop_end_beats = end;
        true
    }

    /// Timeline-local occurrences of one source note onset, including repeats.
    pub(crate) fn note_occurrences(&self, source_beat: f64) -> Vec<f64> {
        self.timeline().occurrences_of(source_beat).collect()
    }
}

#[cfg(test)]
impl UiClip {
    /// A silent, unlooped audio Clip `duration` frames long at `position`,
    /// for tests to fill in the fields they care about.
    pub(crate) fn test_clip(position: u64, duration: u64) -> Self {
        Self {
            id: ClipId::new(),
            name: "Audio".into(),
            audio: Arc::new(DecodedAudio {
                channels: vec![vec![0.0; duration as usize]],
                sample_rate: 48_000,
            }),
            source: None,
            position,
            source_offset: 0,
            start_marker: 0,
            duration,
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
            original_audio: None,
        }
    }
}

#[cfg(test)]
impl UiNoteClip {
    /// An empty, unlooped note Clip `duration_beats` long at
    /// `position_beats`, for tests to fill in the fields they care about.
    pub(crate) fn test_clip(position_beats: f64, duration_beats: f64) -> Self {
        Self {
            id: ClipId::new(),
            name: "MIDI".into(),
            position_beats,
            duration_beats,
            notes: Vec::new(),
            selected_notes: Default::default(),
            start_marker_beats: 0.0,
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: duration_beats,
            groove_grid: Default::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ClipboardClip {
    Audio {
        track_id: TrackId,
        track_offset: usize,
        position_beats: f64,
        clip: UiClip,
    },
    Note {
        track_id: TrackId,
        track_offset: usize,
        position_beats: f64,
        clip: UiNoteClip,
    },
}

#[derive(Debug, Clone, Default)]
pub struct ClipClipboard {
    pub clips: Vec<ClipboardClip>,
}

/// A project-wide track/channel as represented in the UI.
///
/// Musical timeline content deliberately does not live here. Arrange (and,
/// later, each Perform Section) associates its own [`TrackTimelineContent`]
/// with this track's stable [`TrackId`].
#[derive(Debug, Clone)]
pub struct ProjectTrack {
    pub id: TrackId,
    pub name: String,
    pub gain: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    pub audio_input_route: AudioInputRoute,
    pub input_monitoring: InputMonitoring,
    pub swing_offset: Option<vibez_core::perform::SwingOffset>,
    pub peak_l: f32,
    pub peak_r: f32,
    pub effects: Vec<UiEffect>,
    /// Post-fader send amounts into buses: `(bus id, 0..1)`.
    pub sends: Vec<(TrackId, f32)>,
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
    pub instrument_latency_samples: Option<u32>,
}

/// Compatibility name used by older view-only modules. It resolves to the
/// project-owned channel model and cannot carry Arrange Timeline Content.
pub type UiTrack = ProjectTrack;

impl ProjectTrack {
    pub fn has_device_room(&self) -> bool {
        self.effects.len() < vibez_engine::mixer::DEVICE_CHAIN_CAPACITY
    }

    pub fn is_playable_midi_target(&self) -> bool {
        self.kind.is_midi() && self.has_instrument
    }

    pub fn new(id: TrackId, name: String, color_index: u8) -> Self {
        Self {
            id,
            name,
            gain: 1.0,
            pan: 0.5,
            mute: false,
            solo: false,
            audio_input_route: AudioInputRoute::default(),
            input_monitoring: InputMonitoring::default(),
            swing_offset: None,
            peak_l: 0.0,
            peak_r: 0.0,
            effects: Vec::new(),
            sends: Vec::new(),
            kind: TrackKind::Audio,
            color_index,
            has_instrument: false,
            instrument_kind: None,
            sample_name: None,
            sample_source: None,
            sample_audio: None,
            instrument_params: Vec::new(),
            drum_rack_pads: default_drum_rack_pads(),
            selected_drum_pad: 0,
            plugin_instrument_name: None,
            plugin_instrument_ref: None,
            plugin_instrument_descriptors: &[],
            has_plugin_instrument_gui: false,
            instrument_latency_samples: None,
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
            gain: 1.0,
            pan: 0.5,
            mute: false,
            solo: false,
            audio_input_route: AudioInputRoute::default(),
            input_monitoring: InputMonitoring::default(),
            swing_offset: None,
            peak_l: 0.0,
            peak_r: 0.0,
            effects: Vec::new(),
            sends: Vec::new(),
            kind,
            color_index,
            has_instrument,
            instrument_kind,
            sample_name: None,
            sample_source: None,
            sample_audio: None,
            instrument_params: Vec::new(),
            drum_rack_pads: default_drum_rack_pads(),
            selected_drum_pad: 0,
            plugin_instrument_name: None,
            plugin_instrument_ref: None,
            plugin_instrument_descriptors: &[],
            has_plugin_instrument_gui: false,
            instrument_latency_samples: has_instrument.then_some(0),
        }
    }
}

/// Arrange-owned musical content for one shared Project Track.
///
/// This type contains only timeline-editable state. Channel identity,
/// devices, routing, sends, and mixer state stay on [`ProjectTrack`]. Runtime
/// meters and device-media caches therefore cannot leak into timeline clones.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackTimelineContent {
    pub clips: Vec<UiClip>,
    pub note_clips: Vec<UiNoteClip>,
    pub automation: Vec<vibez_core::automation::AutomationLane>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArrangementSelection {
    AudioClip { track_id: TrackId, clip_id: ClipId },
    NoteClip { track_id: TrackId, clip_id: ClipId },
}

impl ArrangementSelection {
    pub const fn track_id(self) -> TrackId {
        match self {
            Self::AudioClip { track_id, .. } | Self::NoteClip { track_id, .. } => track_id,
        }
    }

    pub const fn clip_id(self) -> ClipId {
        match self {
            Self::AudioClip { clip_id, .. } | Self::NoteClip { clip_id, .. } => clip_id,
        }
    }

    pub const fn is_note_clip(self) -> bool {
        matches!(self, Self::NoteClip { .. })
    }
}

/// Drawable geometry for a rubber-band being dragged across a timeline.
///
/// `top_y` / `bottom_y` are measured from the top of the first track row,
/// so every lane can draw its own slice of the same box regardless of
/// where it sits in the column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArrangementMarqueeRect {
    pub start_beats: f64,
    pub end_beats: f64,
    pub top_y: f32,
    pub bottom_y: f32,
}

/// Domain state for an in-flight additive selection gesture.
#[derive(Debug, Clone, PartialEq)]
pub struct ArrangementMarquee {
    pub rect: ArrangementMarqueeRect,
    /// Clip selection as it stood when the gesture began. A shift-drag
    /// unions the covered clips onto this rather than replacing it, so
    /// shrinking the box gives back what it never covered.
    pub base: std::collections::HashSet<ArrangementSelection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Workspace {
    Arrange,
    Perform,
    Mix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailPanelTab {
    Clip,
    Devices,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

    /// The steps a zoom-adaptive grid picks from, coarsest first.
    const ADAPTIVE_STEPS: [Self; 8] = [
        Self::EIGHT_BARS,
        Self::FOUR_BARS,
        Self::TWO_BARS,
        Self::BAR,
        Self::QUARTER,
        Self::EIGHTH,
        Self::SIXTEENTH,
        Self::THIRTY_SECOND,
    ];

    /// The furthest a bias can push the adaptive grid. Any further and every
    /// zoom lands on the same end step.
    pub const ADAPTIVE_BIAS_LIMIT: i8 = Self::ADAPTIVE_STEPS.len() as i8 - 1;

    pub fn adaptive(pixels_per_beat: f32, bias: i8, triplet: bool) -> Self {
        let steps = Self::ADAPTIVE_STEPS;
        let base = steps
            .iter()
            .rposition(|grid| grid.beat_size() * pixels_per_beat as f64 >= 12.0)
            .unwrap_or(0);
        let index = (base as i8 + bias).clamp(0, Self::ADAPTIVE_BIAS_LIMIT) as usize;
        let grid = steps[index];
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

    /// Snap a beat value down to the start of the grid cell containing it.
    ///
    /// Creation gestures must land in the cell the pointer is actually
    /// over. `snap_beat` rounds to the nearest line, so a click in the
    /// right half of a cell would create the note one cell too far right.
    pub fn snap_beat_floor(self, beat: f64) -> f64 {
        let size = self.beat_size();
        (beat / size).floor() * size
    }
}

impl std::fmt::Display for SnapGrid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// The shortest an edge drag may leave a Clip or a note is the finest grid
/// step, whatever grid is showing, because a coarse adaptive grid at fit
/// zoom would otherwise forbid anything under 8 bars.
pub const SHORTEST_LENGTH_BEATS: f64 = 0.125;

/// Grid modes passed into arrangement and clip editor widgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GridConfig {
    pub selected: SnapGrid,
    pub snap_enabled: bool,
    pub adaptive: bool,
    #[serde(deserialize_with = "deserialize_adaptive_bias")]
    pub adaptive_bias: i8,
}

fn deserialize_adaptive_bias<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<i8, D::Error> {
    let bias = <i8 as serde::Deserialize>::deserialize(deserializer)?;
    Ok(bias.clamp(
        -SnapGrid::ADAPTIVE_BIAS_LIMIT,
        SnapGrid::ADAPTIVE_BIAS_LIMIT,
    ))
}

/// Zoom-adaptive like Ableton, because a fixed default is under a pixel
/// per step at song zoom and over-coarse when zoomed in. `selected` is the
/// value a switch to a fixed grid starts from.
impl Default for GridConfig {
    fn default() -> Self {
        Self::new(SnapGrid::EIGHTH, true, true, 0)
    }
}

/// One edit to a [`GridConfig`], from a snap list, toggle or shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridCommand {
    Choose(GridChoice),
    Narrow,
    Widen,
    ToggleTriplet,
    ToggleSnap,
    ToggleAdaptive,
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

    pub fn apply(&mut self, command: GridCommand) {
        match command {
            GridCommand::Choose(choice) => self.choose(choice),
            GridCommand::Narrow => self.narrow(),
            GridCommand::Widen => self.widen(),
            GridCommand::ToggleTriplet => self.toggle_triplet(),
            GridCommand::ToggleSnap => self.toggle_snap(),
            GridCommand::ToggleAdaptive => self.toggle_adaptive(),
        }
    }

    pub fn narrow(&mut self) {
        if self.adaptive {
            self.adaptive_bias = (self.adaptive_bias + 1).min(SnapGrid::ADAPTIVE_BIAS_LIMIT);
        } else {
            self.selected = self.selected.narrower();
        }
    }

    pub fn widen(&mut self) {
        if self.adaptive {
            self.adaptive_bias = (self.adaptive_bias - 1).max(-SnapGrid::ADAPTIVE_BIAS_LIMIT);
        } else {
            self.selected = self.selected.wider();
        }
    }

    pub fn toggle_triplet(&mut self) {
        self.selected = self.selected.toggle_triplet();
    }

    pub fn toggle_snap(&mut self) {
        self.snap_enabled = !self.snap_enabled;
    }

    pub fn toggle_adaptive(&mut self) {
        self.adaptive = !self.adaptive;
    }

    pub fn choose(&mut self, choice: GridChoice) {
        match choice {
            // The step Auto shows is the one this grid's bias resolves to
            // now, so keeping the bias is choosing that step.
            GridChoice::Auto(_) => self.adaptive = true,
            GridChoice::Fixed(grid) => {
                self.selected = grid;
                self.adaptive = false;
                self.adaptive_bias = 0;
            }
        }
    }

    /// The snap list entry for this grid at this zoom.
    pub fn choice(self, pixels_per_beat: f32) -> GridChoice {
        if self.adaptive {
            self.auto_choice(pixels_per_beat)
        } else {
            GridChoice::Fixed(self.selected)
        }
    }

    /// Every snap list entry, Auto first.
    pub fn choices(self, pixels_per_beat: f32) -> Vec<GridChoice> {
        std::iter::once(self.auto_choice(pixels_per_beat))
            .chain(SnapGrid::all().iter().copied().map(GridChoice::Fixed))
            .collect()
    }

    /// The grid menu's fixed entries, coarsest first. They keep the grid's
    /// triplet feel, as Ableton's Fixed Grid menu keeps Triplet Grid on.
    pub fn fixed_choices(self) -> impl Iterator<Item = GridChoice> {
        let triplet = self.selected.is_triplet();
        SnapGrid::ADAPTIVE_STEPS
            .into_iter()
            .map(move |grid| GridChoice::Fixed(if triplet { grid.triplet() } else { grid }))
    }

    fn auto_choice(self, pixels_per_beat: f32) -> GridChoice {
        GridChoice::Auto(SnapGrid::adaptive(
            pixels_per_beat,
            self.adaptive_bias,
            self.selected.is_triplet(),
        ))
    }

    pub fn effective_grid(self, pixels_per_beat: f32) -> SnapGrid {
        match self.choice(pixels_per_beat) {
            GridChoice::Auto(grid) | GridChoice::Fixed(grid) => grid,
        }
    }

    pub fn snap_beat(self, beat: f64, pixels_per_beat: f32) -> f64 {
        if self.snap_enabled {
            self.effective_grid(pixels_per_beat).snap_beat(beat)
        } else {
            beat
        }
    }

    /// Cell-containing snap for creation gestures. See
    /// [`SnapGrid::snap_beat_floor`].
    pub fn snap_beat_floor(self, beat: f64, pixels_per_beat: f32) -> f64 {
        if self.snap_enabled {
            self.effective_grid(pixels_per_beat).snap_beat_floor(beat)
        } else {
            beat
        }
    }
}

/// An entry in an editor's snap list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridChoice {
    /// Follows the editor's zoom; carries the step it resolves to now so
    /// the list reads what a drag will snap to.
    Auto(SnapGrid),
    Fixed(SnapGrid),
}

impl std::fmt::Display for GridChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auto(grid) => write!(f, "Auto {}", grid.label()),
            Self::Fixed(grid) => f.write_str(grid.label()),
        }
    }
}

#[cfg(test)]
#[path = "ui_types_tests.rs"]
mod snap_grid_tests;
