//! The static layer of a clip lane, drawn once and kept until something it
//! shows changes, so playback and drags redraw only what moves.

use std::cell::{Cell, RefCell};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use iced::widget::canvas;
use iced::{mouse, Rectangle, Renderer};
use vibez_core::id::ClipId;

use crate::theme;

use super::{NoteSummary, TimelineClip, TimelineNoteClip, TrackClipCanvas};

/// Canvas state for a lane's static layer.
#[derive(Debug, Default)]
pub struct StaticLayer {
    cache: canvas::Cache,
    /// The layer key and theme epoch the cache was drawn for.
    drawn_for: Cell<(u64, u64)>,
    /// What the key hashed by address. Holding them stops a new waveform or
    /// note summary from reusing an address and passing for unchanged.
    held: RefCell<HeldContent>,
}

#[derive(Debug, Default)]
struct HeldContent {
    _peaks: Vec<Arc<Vec<(f32, f32)>>>,
    _notes: Vec<Arc<NoteSummary>>,
}

impl StaticLayer {
    pub(super) fn draw(
        &self,
        lane: &TrackClipCanvas,
        renderer: &Renderer,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        draw: impl FnOnce(&mut canvas::Frame),
    ) -> canvas::Geometry {
        let drawn_for = (lane.static_layer_key(bounds, cursor), theme::epoch());
        if self.drawn_for.get() != drawn_for {
            self.cache.clear();
            self.drawn_for.set(drawn_for);
            *self.held.borrow_mut() = HeldContent {
                _peaks: lane
                    .resting_clips()
                    .map(|clip| Arc::clone(&clip.peaks))
                    .collect(),
                _notes: lane
                    .resting_note_clips()
                    .map(|clip| Arc::clone(&clip.notes))
                    .collect(),
            };
        }
        self.cache.draw(renderer, bounds.size(), draw)
    }
}

impl TrackClipCanvas {
    /// Whether a Clip moves with the drag in flight: the anchor and, as a
    /// group moves together, every selected Clip.
    pub(super) fn is_moving(&self, clip_id: ClipId) -> bool {
        self.is_dragged(clip_id)
            || (self.clip_drag.is_some() && self.selected_clips.contains(&clip_id))
    }

    pub(super) fn resting_clips(&self) -> impl Iterator<Item = &TimelineClip> {
        self.clips
            .iter()
            .filter(|clip| !self.is_moving(clip.clip_id))
    }

    pub(super) fn resting_note_clips(&self) -> impl Iterator<Item = &TimelineNoteClip> {
        self.note_clips
            .iter()
            .filter(|clip| !self.is_moving(clip.clip_id))
    }

    /// Everything the static layer draws, besides the theme, hashed so the
    /// layer is drawn again only when one of them changes.
    pub(super) fn static_layer_key(&self, bounds: Rectangle, cursor: mouse::Cursor) -> u64 {
        let mut hasher = DefaultHasher::new();
        let ppb = self.pixels_per_beat();
        (
            // The whole-pixel shift depends on where layout put the lane.
            bounds.x.fract().to_bits(),
            bounds.y.fract().to_bits(),
            bounds.width.to_bits(),
            bounds.height.to_bits(),
            self.zoom_level.to_bits(),
            self.scroll_offset_beats.to_bits(),
            self.grid.effective_grid(ppb).beat_size().to_bits(),
            self.bpm.to_bits(),
            self.sample_rate,
        )
            .hash(&mut hasher);
        (
            self.loop_enabled,
            self.loop_start_beats.to_bits(),
            self.loop_end_beats.to_bits(),
        )
            .hash(&mut hasher);
        (
            self.selected,
            self.muted,
            self.is_instrument,
            self.drop_hover(bounds, cursor),
            [
                self.track_color.r,
                self.track_color.g,
                self.track_color.b,
                self.track_color.a,
            ]
            .map(f32::to_bits),
        )
            .hash(&mut hasher);
        for clip in self.resting_clips() {
            (
                clip.clip_id,
                clip.position,
                clip.duration,
                &clip.name,
                Arc::as_ptr(&clip.peaks) as usize,
                clip.peak_span_frames,
                clip.loop_enabled,
                clip.loop_start,
                clip.loop_end,
                self.selected_clips.contains(&clip.clip_id),
            )
                .hash(&mut hasher);
            (
                clip.fade_in_frames,
                clip.fade_out_frames,
                clip.fade_in_curve,
                clip.fade_out_curve,
                clip.crossfade_in_from,
                clip.crossfade_out_to,
                clip.warp_stale,
            )
                .hash(&mut hasher);
        }
        for clip in self.resting_note_clips() {
            (
                clip.clip_id,
                clip.position_beats.to_bits(),
                clip.duration_beats.to_bits(),
                &clip.name,
                Arc::as_ptr(&clip.notes) as usize,
                clip.start_marker_beats.to_bits(),
                clip.loop_enabled,
                clip.loop_start_beats.to_bits(),
                clip.loop_end_beats.to_bits(),
                self.selected_clips.contains(&clip.clip_id),
            )
                .hash(&mut hasher);
        }
        hasher.finish()
    }
}
