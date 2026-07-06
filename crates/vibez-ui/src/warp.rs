//! Clip warp math: tempo-matching a clip's audio to the project BPM.
//!
//! The stretch itself lives in `vibez_dsp::time_stretch`. This module
//! owns the surrounding arithmetic, which has two invariants that were
//! violated by earlier revisions (dogfood session 2026-07-03, bugs #7
//! and #8 in `.notes/dogfood-2026-07-03.md`):
//!
//! 1. **Re-warp must be idempotent.** A clip's geometry fields
//!    (`duration`, `source_offset`, `loop_start`, `loop_end`) are
//!    expressed in samples of whatever buffer the clip currently
//!    references. When re-warping we stretch from the retained
//!    *original* audio, so geometry must be rescaled by
//!    `new_target / fields_frames` (the buffer the fields refer to),
//!    NOT by `new_target / original_frames`. Scaling by the latter
//!    compounds the ratio on every warp and drifts the clip
//!    boundaries off the audio content.
//!
//! 2. **The warp is deterministic.** `warp_target_frames` is a pure
//!    function of (source frames, sample rate, clip BPM, project BPM),
//!    so project load can regenerate the exact stretched buffer the
//!    saved geometry refers to by re-running it with the persisted
//!    `original_bpm` / `warped_to_bpm` pair.

use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;

use crate::message::ClipWarpSuccess;

pub struct WarpClipInput {
    /// The un-warped source audio to stretch.
    pub audio: Arc<DecodedAudio>,
    /// Frame count of the buffer the clip's geometry fields currently
    /// reference. Equal to `audio.num_frames()` on a first warp;
    /// equal to the current (already-warped) buffer's frame count on
    /// a re-warp.
    pub fields_frames: u64,
    pub source_offset: u64,
    pub duration: u64,
    pub loop_start: u64,
    pub loop_end: u64,
    pub clip_bpm: f64,
    pub project_bpm: f64,
}

/// Deterministic warped length for a clip: the naive proportional
/// length snapped to the nearest whole bar at the project tempo when
/// the naive result lands close to an integer bar count.
///
/// Real drum loops almost always carry a trailing pickup (the "and"
/// before beat 1 of the next bar) for crossfade purposes. A naive
/// proportional warp preserves that pickup, which then leaks past the
/// arrangement loop boundary and sounds like a double beat. Snapping
/// to the nearest bar trims / extends the clip so a "1 bar" loop
/// plays as exactly one bar at the project tempo.
///
/// The 0.25-bar guard means we only snap when the source is plausibly
/// "about N bars"; a 1.5-bar clip stays at 1.5 bars rather than being
/// aggressively remapped.
pub fn warp_target_frames(
    source_frames: usize,
    sample_rate: f64,
    clip_bpm: f64,
    project_bpm: f64,
) -> usize {
    // Same number of beats at a different tempo:
    //   source_frames = beats * 60 / clip_bpm * sample_rate
    //   target_frames = beats * 60 / project_bpm * sample_rate
    //   => target / source = clip_bpm / project_bpm
    let naive_ratio = clip_bpm / project_bpm;
    let naive_target = source_frames as f64 * naive_ratio;

    let bar_samples = 4.0 * 60.0 * sample_rate / project_bpm;
    if bar_samples >= 1.0 {
        let naive_bars = naive_target / bar_samples;
        if naive_bars >= 0.5 {
            let rounded = naive_bars.round();
            if (naive_bars - rounded).abs() < 0.25 {
                return (rounded * bar_samples).round() as usize;
            }
        }
    }
    naive_target as usize
}

pub fn compute_warp(input: WarpClipInput) -> Result<ClipWarpSuccess, String> {
    if input.clip_bpm <= 0.0 || input.project_bpm <= 0.0 {
        return Err("Invalid BPM".into());
    }
    let source_frames = input.audio.num_frames();
    if source_frames == 0 {
        return Err("Empty audio".into());
    }
    if input.fields_frames == 0 {
        return Err("Empty clip geometry reference".into());
    }
    let sample_rate = input.audio.sample_rate as f64;
    if sample_rate <= 0.0 {
        return Err("Invalid sample rate".into());
    }

    let target_total = warp_target_frames(
        source_frames,
        sample_rate,
        input.clip_bpm,
        input.project_bpm,
    );
    if target_total == 0 {
        return Err("Target length collapsed to zero".into());
    }

    let stretched = vibez_dsp::time_stretch::pitch_preserving_stretch(&input.audio, target_total);
    // Geometry fields are rescaled relative to the buffer they
    // currently reference, so warping an already-warped clip to the
    // same tempo is a no-op on geometry and warping to a new tempo
    // lands exactly where a fresh warp from the original would.
    let field_ratio = target_total as f64 / input.fields_frames as f64;
    let scale = |x: u64| -> u64 { (x as f64 * field_ratio).round() as u64 };
    Ok(ClipWarpSuccess {
        audio: Arc::new(stretched),
        original_audio: input.audio,
        new_duration: scale(input.duration),
        new_source_offset: scale(input.source_offset),
        new_loop_start: scale(input.loop_start),
        new_loop_end: scale(input.loop_end),
        detected_bpm: input.clip_bpm,
        warped_to_bpm: input.project_bpm,
    })
}

pub async fn warp_clip_async(input: WarpClipInput) -> Result<ClipWarpSuccess, String> {
    tokio::task::spawn_blocking(move || compute_warp(input))
        .await
        .map_err(|e| format!("warp task failed: {e}"))?
}

/// Regenerate the stretched audio a saved warped clip's geometry
/// refers to. Used on project load: the project file stores the raw
/// source reference plus geometry in warped-sample units, so the
/// stretch has to be re-applied before the clip is handed to the
/// engine. Deterministic: same inputs as the original warp produce
/// the same target length.
pub fn rewarp_for_load(
    raw: &Arc<DecodedAudio>,
    clip_bpm: f64,
    warped_to_bpm: f64,
) -> Option<Arc<DecodedAudio>> {
    if clip_bpm <= 0.0 || warped_to_bpm <= 0.0 {
        return None;
    }
    let source_frames = raw.num_frames();
    if source_frames == 0 {
        return None;
    }
    let sample_rate = raw.sample_rate as f64;
    if sample_rate <= 0.0 {
        return None;
    }
    let target = warp_target_frames(source_frames, sample_rate, clip_bpm, warped_to_bpm);
    if target == 0 {
        return None;
    }
    Some(Arc::new(vibez_dsp::time_stretch::pitch_preserving_stretch(
        raw, target,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 44_100;

    /// A synthetic loop that is exactly `bars` bars long at `bpm`.
    fn exact_loop(bars: f64, bpm: f64) -> Arc<DecodedAudio> {
        let frames = (bars * 4.0 * 60.0 / bpm * SR as f64).round() as usize;
        Arc::new(DecodedAudio {
            channels: vec![vec![0.25; frames]],
            sample_rate: SR,
        })
    }

    fn first_warp_input(
        audio: &Arc<DecodedAudio>,
        clip_bpm: f64,
        project_bpm: f64,
    ) -> WarpClipInput {
        let frames = audio.num_frames() as u64;
        WarpClipInput {
            audio: Arc::clone(audio),
            fields_frames: frames,
            source_offset: 0,
            duration: frames,
            loop_start: 0,
            loop_end: frames,
            clip_bpm,
            project_bpm,
        }
    }

    #[test]
    fn warp_128_to_120_lengthens_to_exact_bars() {
        // 2-bar loop at 128 warped into a 120 project must come out
        // exactly 2 bars long at 120: 2 * 4 * 60/120 * 44100 = 176400.
        let audio = exact_loop(2.0, 128.0);
        let target = warp_target_frames(audio.num_frames(), SR as f64, 128.0, 120.0);
        assert_eq!(target, 176_400);
    }

    #[test]
    fn warp_direction_slower_project_lengthens_faster_project_shortens() {
        let audio = exact_loop(2.0, 128.0);
        let src = audio.num_frames();
        let slower = warp_target_frames(src, SR as f64, 128.0, 120.0);
        let faster = warp_target_frames(src, SR as f64, 128.0, 140.0);
        assert!(slower > src, "128->120 must lengthen: {src} -> {slower}");
        assert!(faster < src, "128->140 must shorten: {src} -> {faster}");
    }

    #[test]
    fn bar_snap_leaves_non_integer_bar_sources_alone() {
        // A 1.5-bar source is not "about N bars"; the naive
        // proportional length must be preserved.
        let audio = exact_loop(1.5, 128.0);
        let src = audio.num_frames();
        let target = warp_target_frames(src, SR as f64, 128.0, 120.0);
        let naive = (src as f64 * 128.0 / 120.0) as usize;
        assert_eq!(target, naive);
    }

    #[test]
    fn first_warp_scales_geometry_by_ratio() {
        let audio = exact_loop(2.0, 128.0);
        let out = compute_warp(first_warp_input(&audio, 128.0, 120.0)).unwrap();
        assert_eq!(out.audio.num_frames(), 176_400);
        assert_eq!(out.new_duration, 176_400);
        assert_eq!(out.new_loop_end, 176_400);
        assert_eq!(out.new_source_offset, 0);
    }

    /// Regression: dogfood 2026-07-03 bug #8. Re-warping to the SAME
    /// tempo must not move the clip geometry. The old code rescaled
    /// warped-unit fields by the full original->target ratio again,
    /// compounding ~6.7% per warp at 128->120.
    #[test]
    fn rewarp_to_same_tempo_is_idempotent() {
        let audio = exact_loop(2.0, 128.0);
        let first = compute_warp(first_warp_input(&audio, 128.0, 120.0)).unwrap();

        let second = compute_warp(WarpClipInput {
            audio: Arc::clone(&first.original_audio),
            fields_frames: first.audio.num_frames() as u64,
            source_offset: first.new_source_offset,
            duration: first.new_duration,
            loop_start: first.new_loop_start,
            loop_end: first.new_loop_end,
            clip_bpm: 128.0,
            project_bpm: 120.0,
        })
        .unwrap();

        assert_eq!(second.new_duration, first.new_duration);
        assert_eq!(second.new_loop_end, first.new_loop_end);
        assert_eq!(second.audio.num_frames(), first.audio.num_frames());
        // Geometry must match the audio it plays.
        assert_eq!(second.new_duration as usize, second.audio.num_frames());
    }

    /// Regression: re-warping to a NEW tempo must land exactly where
    /// a fresh warp from the original would, with geometry matching
    /// the stretched audio.
    #[test]
    fn rewarp_to_new_tempo_matches_fresh_warp() {
        let audio = exact_loop(2.0, 128.0);
        let first = compute_warp(first_warp_input(&audio, 128.0, 120.0)).unwrap();

        let rewarped = compute_warp(WarpClipInput {
            audio: Arc::clone(&first.original_audio),
            fields_frames: first.audio.num_frames() as u64,
            source_offset: first.new_source_offset,
            duration: first.new_duration,
            loop_start: first.new_loop_start,
            loop_end: first.new_loop_end,
            clip_bpm: 128.0,
            project_bpm: 135.0,
        })
        .unwrap();

        let fresh = compute_warp(first_warp_input(&audio, 128.0, 135.0)).unwrap();
        assert_eq!(rewarped.audio.num_frames(), fresh.audio.num_frames());
        assert_eq!(rewarped.new_duration, fresh.new_duration);
        assert_eq!(rewarped.new_loop_end, fresh.new_loop_end);
    }

    /// Regression: dogfood 2026-07-03 bug #7. Project load must be
    /// able to regenerate a stretched buffer whose length matches the
    /// warped-unit geometry that was saved.
    #[test]
    fn load_rewarp_reproduces_saved_geometry_length() {
        let audio = exact_loop(2.0, 128.0);
        let saved = compute_warp(first_warp_input(&audio, 128.0, 120.0)).unwrap();

        // Simulate reload: raw audio decoded from disk + persisted BPM pair.
        let reloaded = rewarp_for_load(&saved.original_audio, 128.0, 120.0).unwrap();
        assert_eq!(reloaded.num_frames() as u64, saved.new_duration);
    }

    #[test]
    fn load_rewarp_rejects_bad_inputs() {
        let audio = exact_loop(2.0, 128.0);
        assert!(rewarp_for_load(&audio, 0.0, 120.0).is_none());
        assert!(rewarp_for_load(&audio, 128.0, 0.0).is_none());
        let empty = Arc::new(DecodedAudio {
            channels: vec![Vec::new()],
            sample_rate: SR,
        });
        assert!(rewarp_for_load(&empty, 128.0, 120.0).is_none());
    }
}
