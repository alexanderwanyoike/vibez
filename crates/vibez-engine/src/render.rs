//! Offline rendering (bounce / resample).
//!
//! Builds a fresh set of [`EngineTrack`]s from a project snapshot, drives them
//! block-by-block without an audio I/O stream, and returns a
//! [`DecodedAudio`] buffer. The same mixer and instrument paths that run on
//! the audio thread are used, so what you bounce matches what you hear.
//!
//! Third-party plugin instances are prepared by the UI (their main-thread
//! initialization cannot happen in this crate) and supplied to
//! [`render_offline_with_plugins`]. A declared plugin is never silently
//! substituted or skipped by that strict export path.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::{ClipId, EffectId, TrackId};
use vibez_core::midi::NoteClipInfo;
use vibez_core::time::TempoMap;
use vibez_core::track::{ClipInfo, InstrumentStateInfo, TrackInfo};
use vibez_dsp::factory::create_effect_with_params;
use vibez_instruments::create_instrument;

use crate::mixer::{
    any_solo, equal_power_pan, EffectSlot, EngineClip, EngineNoteClip, EngineTrack,
    InstrumentRenderContext,
};

/// What to render offline.
#[derive(Debug, Clone, Copy)]
pub enum BounceMode {
    /// Sum of all non-muted / soloed tracks. Mute and solo rules mirror live
    /// playback.
    Master,
    /// Render one track post-effects with its gain/pan. Mute/solo on the
    /// target are ignored so you always hear the thing you asked for.
    Track(TrackId),
    /// Render a single clip on one track. Other clips on the same track and
    /// all other tracks are excluded.
    Clip {
        track_id: TrackId,
        clip_id: ClipId,
        is_note_clip: bool,
    },
}

/// Snapshot of the parts of a project needed to render offline, plus the
/// decoded audio for every asset referenced by the snapshot. The live Audition
/// Bus is intentionally absent, so exports, stems, and resampling cannot capture
/// Browser playback.
pub struct BounceRequest {
    pub tracks: Vec<TrackInfo>,
    /// Master bus (gain + effect chain), applied to the summed mix
    /// in [`BounceMode::Master`] renders.
    pub master: Option<TrackInfo>,
    /// Return buses; fed by track sends in [`BounceMode::Master`].
    pub buses: Vec<TrackInfo>,
    pub audio_clips: Vec<ClipInfo>,
    pub note_clips: Vec<NoteClipInfo>,
    pub clip_audio: HashMap<ClipId, Arc<DecodedAudio>>,
    pub sampler_audio: HashMap<TrackId, (Arc<DecodedAudio>, String)>,
    pub drum_pad_audio: HashMap<(TrackId, usize), (Arc<DecodedAudio>, String)>,
    pub mode: BounceMode,
    /// Render window in absolute samples: `[start, end)` at `sample_rate`.
    pub range_samples: (u64, u64),
    pub bpm: f64,
    pub sample_rate: u32,
    pub swing: vibez_core::perform::SwingAmount,
}

pub struct BounceResult {
    pub audio: DecodedAudio,
    pub warnings: Vec<String>,
}

/// Isolated third-party devices prepared for one offline render.
///
/// Keys are project ids, so each declared slot consumes exactly the instance
/// prepared for it. Missing devices are fatal in the strict export path.
#[derive(Default)]
pub struct OfflinePlugins {
    pub instruments: HashMap<TrackId, Box<dyn vibez_instruments::Instrument>>,
    pub effects: HashMap<EffectId, Box<dyn vibez_dsp::effect::AudioEffect>>,
}

const BLOCK_FRAMES: usize = 512;
const CHANNELS: usize = 2;

/// Render the request and return interleaved stereo [`DecodedAudio`] at the
/// project's sample rate.
pub fn render_offline(req: &BounceRequest) -> BounceResult {
    render_offline_inner(req, None, |_| {})
        .expect("the compatibility renderer cannot fail without strict plugin preparation")
}

/// Strict production renderer used by project export.
///
/// Every plugin declared by the snapshot must have a matching isolated
/// instance in `plugins`. `progress` receives monotonic percentages from
/// 0 through 100.
pub fn render_offline_with_plugins(
    req: &BounceRequest,
    plugins: &mut OfflinePlugins,
    progress: impl FnMut(u8),
) -> Result<BounceResult, String> {
    validate_offline_plugins(req, plugins)?;
    render_offline_inner(req, Some(plugins), progress)
}

#[path = "render_execute.rs"]
mod execute;
#[path = "render_prepare.rs"]
mod prepare;
use execute::render_offline_inner;
use prepare::{
    clip_included_for_mode, return_offline_plugins, return_plugin_effects,
    track_is_active_for_mode, validate_offline_plugins,
};
#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
