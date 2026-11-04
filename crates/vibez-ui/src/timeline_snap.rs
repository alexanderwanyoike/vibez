//! Magnetic snap: the grid line, clip edge or marker a timeline drag or
//! click locks onto.

use crate::state::{ArrangementSelection, GridConfig, TimelineEditorState};

/// How close, in screen pixels, a clip edge or marker must be to pull an
/// edge onto it. Fixed on screen so the pull feels the same at every zoom.
pub const MAGNET_RADIUS_PX: f32 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapTargetKind {
    Grid,
    ClipEdge,
    Playhead,
    LoopStart,
    LoopEnd,
    TimeSelectionEdge,
}

/// A line a gesture locked onto.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnapTarget {
    pub beat: f64,
    pub kind: SnapTargetKind,
}

/// Where a gesture lands, and the line it locked onto (`None` when it
/// moved freely).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snapped {
    pub beat: f64,
    pub target: Option<SnapTarget>,
}

/// The grid and zoom a gesture snaps at. `free` bypasses all snapping, as
/// holding Alt does in Ableton.
#[derive(Debug, Clone, Copy)]
pub struct SnapContext {
    pub grid: GridConfig,
    pub pixels_per_beat: f32,
    pub free: bool,
}

#[derive(Debug, Clone, Copy)]
struct Magnet {
    target: SnapTarget,
    /// An edge of a selected Clip, which moves with any drag.
    dragged: bool,
}

/// The clip edges and markers of one timeline that drags and clicks lock
/// onto besides the grid.
#[derive(Debug, Clone, Default)]
pub struct SnapTargets {
    magnets: Vec<Magnet>,
}

impl SnapTargets {
    /// Every Clip edge on `editor`'s timeline, and its time selection.
    pub fn for_editor(editor: &TimelineEditorState, samples_per_beat: f64) -> Self {
        let mut targets = Self::default();
        for (&track_id, content) in &editor.timeline.by_track {
            let audio = content.clips.iter().map(|clip| {
                (
                    ArrangementSelection::AudioClip {
                        track_id,
                        clip_id: clip.id,
                    },
                    clip.span(samples_per_beat),
                )
            });
            let notes = content.note_clips.iter().map(|clip| {
                (
                    ArrangementSelection::NoteClip {
                        track_id,
                        clip_id: clip.id,
                    },
                    clip.span(),
                )
            });
            for (selection, span) in audio.chain(notes) {
                let dragged = editor.selected_clips.contains(&selection);
                for beat in [span.start_beats, span.end_beats()] {
                    targets.magnets.push(Magnet {
                        target: SnapTarget {
                            beat,
                            kind: SnapTargetKind::ClipEdge,
                        },
                        dragged,
                    });
                }
            }
        }
        if editor.time_selection_active {
            for beat in [editor.selection_start_beats, editor.selection_end_beats] {
                targets = targets.with_marker(beat, SnapTargetKind::TimeSelectionEdge);
            }
        }
        targets
    }

    pub fn with_marker(mut self, beat: f64, kind: SnapTargetKind) -> Self {
        self.magnets.push(Magnet {
            target: SnapTarget { beat, kind },
            dragged: false,
        });
        self
    }

    /// Where a click at `beat` puts the insert point. Selected Clips stay
    /// put on a click, so their edges pull too.
    pub fn click(&self, beat: f64, context: SnapContext) -> Snapped {
        self.land(&[beat], Self::grid_shift(beat, context), context, true)
    }

    /// Where a dragged Clip end at `end` lands.
    pub fn dragged_end(&self, end: f64, context: SnapContext) -> Snapped {
        self.land(&[end], Self::grid_shift(end, context), context, false)
    }

    /// Where the start of a span at `start`, `length` long, lands after
    /// `travel` beats of drag. The grid snaps the travel, so an off-grid
    /// span keeps its offset from the grid, while either edge may lock onto
    /// a magnet.
    pub fn moved_span(
        &self,
        start: f64,
        length: f64,
        travel: f64,
        context: SnapContext,
    ) -> Snapped {
        let moved = start + travel;
        self.land(
            &[moved, moved + length],
            Self::grid_shift(travel, context),
            context,
            false,
        )
    }

    fn grid_shift(beat: f64, context: SnapContext) -> f64 {
        context.grid.snap_beat(beat, context.pixels_per_beat) - beat
    }

    /// Shifts `edges` by `grid_shift`, or by less when that locks one of
    /// them onto a magnet within reach, and says where the first one lands.
    fn land(
        &self,
        edges: &[f64],
        grid_shift: f64,
        context: SnapContext,
        with_dragged: bool,
    ) -> Snapped {
        let first = edges[0];
        if context.free || !context.grid.snap_enabled {
            return Snapped {
                beat: first,
                target: None,
            };
        }
        let grid = (
            grid_shift,
            SnapTarget {
                beat: first + grid_shift,
                kind: SnapTargetKind::Grid,
            },
        );
        let reach = f64::from(MAGNET_RADIUS_PX / context.pixels_per_beat);
        let (shift, target) = self
            .magnets
            .iter()
            .filter(|magnet| with_dragged || !magnet.dragged)
            .flat_map(|magnet| {
                edges
                    .iter()
                    .map(move |edge| (magnet.target.beat - edge, magnet.target))
            })
            .filter(|(shift, _)| shift.abs() <= reach)
            .fold(grid, |nearest, pull| {
                if pull.0.abs() <= nearest.0.abs() {
                    pull
                } else {
                    nearest
                }
            });
        Snapped {
            beat: first + shift,
            target: Some(target),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use vibez_core::audio_buffer::DecodedAudio;
    use vibez_core::id::{ClipId, TrackId};

    use super::*;
    use crate::state::{SnapGrid, UiClip, UiNoteClip};

    /// 20 px per beat puts the magnet radius at 0.4 beats.
    const PIXELS_PER_BEAT: f32 = 20.0;
    const SAMPLES_PER_BEAT: f64 = 22_050.0;

    fn context() -> SnapContext {
        SnapContext {
            grid: GridConfig::new(SnapGrid::EIGHTH, true, false, 0),
            pixels_per_beat: PIXELS_PER_BEAT,
            free: false,
        }
    }

    fn note_clip(start: f64, end: f64) -> UiNoteClip {
        UiNoteClip {
            id: ClipId::new(),
            name: "MIDI".into(),
            position_beats: start,
            duration_beats: end - start,
            notes: Vec::new(),
            selected_notes: Default::default(),
            start_marker_beats: 0.0,
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
            groove_grid: Default::default(),
        }
    }

    fn audio_clip(start: f64, end: f64) -> UiClip {
        UiClip {
            id: ClipId::new(),
            name: "Audio".into(),
            audio: Arc::new(DecodedAudio {
                channels: vec![vec![0.0; 8]],
                sample_rate: 44_100,
            }),
            source: None,
            position: (start * SAMPLES_PER_BEAT).round() as u64,
            source_offset: 0,
            start_marker: 0,
            duration: ((end - start) * SAMPLES_PER_BEAT).round() as u64,
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

    /// An editor with an audio Clip spanning 2.1..6.3 on one Track and a
    /// note Clip spanning 10.35..14.0 on another.
    fn editor() -> TimelineEditorState {
        let mut editor = TimelineEditorState::default();
        let timeline = Arc::make_mut(&mut editor.timeline);
        timeline
            .ensure(TrackId::new())
            .clips
            .push(audio_clip(2.1, 6.3));
        timeline
            .ensure(TrackId::new())
            .note_clips
            .push(note_clip(10.35, 14.0));
        editor
    }

    fn targets() -> SnapTargets {
        SnapTargets::for_editor(&editor(), SAMPLES_PER_BEAT)
    }

    fn assert_lands(snapped: Snapped, beat: f64, kind: SnapTargetKind) {
        assert!(
            (snapped.beat - beat).abs() < 1e-9,
            "landed on {} rather than {beat}",
            snapped.beat
        );
        assert_eq!(snapped.target.map(|target| target.kind), Some(kind));
    }

    #[test]
    fn a_clip_edge_inside_the_radius_beats_the_grid() {
        assert_lands(
            targets().click(2.2, context()),
            2.1,
            SnapTargetKind::ClipEdge,
        );
    }

    #[test]
    fn the_grid_wins_outside_the_radius() {
        assert_lands(targets().click(2.8, context()), 3.0, SnapTargetKind::Grid);
    }

    #[test]
    fn a_grid_line_nearer_than_the_edge_wins() {
        assert_lands(targets().click(2.02, context()), 2.0, SnapTargetKind::Grid);
    }

    #[test]
    fn the_radius_is_fixed_on_screen() {
        let bars = SnapContext {
            grid: GridConfig::new(SnapGrid::BAR, true, false, 0),
            ..context()
        };
        let zoomed_in = SnapContext {
            pixels_per_beat: 80.0,
            ..bars
        };

        assert_lands(targets().click(6.6, bars), 6.3, SnapTargetKind::ClipEdge);
        assert_lands(targets().click(6.6, zoomed_in), 8.0, SnapTargetKind::Grid);
    }

    #[test]
    fn alt_bypasses_every_target() {
        let free = SnapContext {
            free: true,
            ..context()
        };

        let snapped = targets().click(2.2, free);

        assert!(
            (snapped.beat - 2.2).abs() < 1e-9,
            "landed on {}",
            snapped.beat
        );
        assert_eq!(snapped.target, None);
    }

    #[test]
    fn a_moved_span_locks_its_end_to_another_clips_start() {
        // A 2 beat span from 3.0 dragged 5.2 beats ends at 10.2, near the
        // note Clip's start at 10.35, so it starts at 8.35.
        let snapped = targets().moved_span(3.0, 2.0, 5.2, context());

        assert_lands(snapped, 8.35, SnapTargetKind::ClipEdge);
        assert!((snapped.target.unwrap().beat - 10.35).abs() < 1e-9);
    }

    #[test]
    fn a_moved_span_off_every_magnet_keeps_its_grid_offset() {
        let snapped = targets().moved_span(14.1, 1.0, 1.1, context());

        assert_lands(snapped, 15.1, SnapTargetKind::Grid);
    }

    #[test]
    fn a_dragged_end_locks_onto_another_clips_start() {
        assert_lands(
            targets().dragged_end(10.25, context()),
            10.35,
            SnapTargetKind::ClipEdge,
        );
    }

    #[test]
    fn drags_ignore_the_selected_clips_they_move_but_clicks_use_them() {
        let mut editor = editor();
        let (&track_id, content) = editor
            .timeline
            .by_track
            .iter()
            .find(|(_, content)| !content.clips.is_empty())
            .unwrap();
        let clip_id = content.clips[0].id;
        editor
            .selected_clips
            .insert(ArrangementSelection::AudioClip { track_id, clip_id });
        let targets = SnapTargets::for_editor(&editor, SAMPLES_PER_BEAT);

        assert_lands(
            targets.dragged_end(2.2, context()),
            2.0,
            SnapTargetKind::Grid,
        );
        assert_lands(targets.click(2.2, context()), 2.1, SnapTargetKind::ClipEdge);
    }

    #[test]
    fn markers_and_the_time_selection_pull_like_clip_edges() {
        let editor = TimelineEditorState {
            time_selection_active: true,
            selection_start_beats: 20.15,
            selection_end_beats: 24.0,
            ..Default::default()
        };
        let targets = SnapTargets::for_editor(&editor, SAMPLES_PER_BEAT)
            .with_marker(30.2, SnapTargetKind::Playhead)
            .with_marker(40.15, SnapTargetKind::LoopEnd);

        assert_lands(
            targets.click(20.3, context()),
            20.15,
            SnapTargetKind::TimeSelectionEdge,
        );
        assert_lands(
            targets.click(30.12, context()),
            30.2,
            SnapTargetKind::Playhead,
        );
        assert_lands(
            targets.click(40.3, context()),
            40.15,
            SnapTargetKind::LoopEnd,
        );
    }
}
