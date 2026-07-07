use vibez_core::id::ClipId;

// ── Lightweight data types for rendering ──

/// Lightweight copy of clip data for rendering.
pub struct TimelineClip {
    pub clip_id: ClipId,
    pub position: u64,
    pub duration: u64,
    pub name: String,
    /// Pre-computed waveform peaks for mini display (per pixel column).
    pub peaks: Vec<(f32, f32)>,
    pub loop_enabled: bool,
    pub loop_start: u64,
    pub loop_end: u64,
    /// True when this clip is warped but its `warped_to_bpm` no longer
    /// matches the current project BPM. The canvas draws a diagonal
    /// stripe overlay so the user can see at a glance that a re-warp
    /// is needed.
    pub warp_stale: bool,
}

/// Lightweight copy of a note clip for timeline rendering.
pub struct TimelineNoteClip {
    pub clip_id: ClipId,
    pub position_beats: f64,
    pub duration_beats: f64,
    pub name: String,
    pub notes: Vec<(u8, f64, f64)>, // (pitch, start_beat, duration_beats)
    pub loop_enabled: bool,
    pub loop_start_beats: f64,
    pub loop_end_beats: f64,
}

/// Compute waveform peaks for a clip, with loop-aware wrapping.
/// Uses `peak_in_range` on contiguous segments for O(pixels) cost regardless of clip length.
pub fn compute_clip_peaks(clip: &crate::state::UiClip) -> Vec<(f32, f32)> {
    let num_peaks = (clip.duration as usize / 100).clamp(1, 1000);
    let looping = clip.loop_enabled && clip.loop_end > clip.loop_start;
    let loop_start = clip.loop_start as usize;
    let loop_end = clip.loop_end as usize;
    let loop_len = if looping { loop_end - loop_start } else { 0 };
    let channels = clip.audio.num_channels();
    if channels == 0 {
        return vec![(0.0, 0.0); num_peaks];
    }

    let peak_for_range = |src_start: usize, src_end: usize| -> (f32, f32) {
        let mut mn = 0.0f32;
        let mut mx = 0.0f32;
        for ch in 0..channels {
            let (ch_min, ch_max) = clip.audio.peak_in_range(ch, src_start, src_end);
            mn = mn.min(ch_min);
            mx = mx.max(ch_max);
        }
        (mn, mx)
    };

    // Cache full loop region peak for spans >= loop_len
    let full_loop_peak = if looping {
        Some(peak_for_range(loop_start, loop_end))
    } else {
        None
    };

    (0..num_peaks)
        .map(|i| {
            let cf_start = i * clip.duration as usize / num_peaks;
            let cf_end = (i + 1) * clip.duration as usize / num_peaks;
            let span = cf_end.saturating_sub(cf_start).max(1);

            if !looping {
                let src_start = clip.source_offset as usize + cf_start;
                let src_end = clip.source_offset as usize + cf_end;
                peak_for_range(src_start, src_end)
            } else if span >= loop_len {
                full_loop_peak.unwrap()
            } else {
                let raw_start = clip.source_offset as usize + cf_start;
                let raw_end = clip.source_offset as usize + cf_end;
                let src_start = if raw_start >= loop_end {
                    loop_start + (raw_start - loop_start) % loop_len
                } else {
                    raw_start
                };
                let src_end = if raw_end >= loop_end {
                    loop_start + (raw_end - loop_start) % loop_len
                } else {
                    raw_end
                };

                if src_start <= src_end {
                    peak_for_range(src_start, src_end.max(src_start + 1))
                } else {
                    // Wraps around loop boundary
                    let (mn1, mx1) = peak_for_range(src_start, loop_end);
                    let (mn2, mx2) = peak_for_range(loop_start, src_end.max(loop_start + 1));
                    (mn1.min(mn2), mx1.max(mx2))
                }
            }
        })
        .collect()
}

// ── RulerWidget ──

/// Pixel threshold for resize handle on right edge of clip.
pub(super) const RESIZE_EDGE_PX: f32 = 8.0;

/// Height of the clip title bar zone (move/resize). Below this is the body zone (seek/region select).
pub(super) const CLIP_TITLE_HEIGHT: f32 = 18.0;
/// Top padding of clips within the track canvas.
pub(super) const CLIP_Y: f32 = 4.0;

mod clips;
mod clips_draw;
mod minimap;
mod ruler;

pub use clips::*;
pub use minimap::*;
pub use ruler::*;
