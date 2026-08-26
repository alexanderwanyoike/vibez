//! Nondestructive piecewise timing maps for Audio Clips.

use serde::{Deserialize, Serialize};

/// One anchor connecting an absolute source frame to a Clip-local timeline
/// frame. The first and last anchors are fixed boundaries; producers edit only
/// the interior anchors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WarpMarker {
    source_frame: u64,
    timeline_frame: u64,
}

impl WarpMarker {
    pub const fn new(source_frame: u64, timeline_frame: u64) -> Self {
        Self {
            source_frame,
            timeline_frame,
        }
    }

    pub const fn source_frame(self) -> u64 {
        self.source_frame
    }

    pub const fn timeline_frame(self) -> u64 {
        self.timeline_frame
    }
}

/// Ordered Warp Markers for one Audio Clip.
///
/// An empty map is the identity mapping from `source_offset + local_frame`.
/// A non-empty map includes both fixed boundary anchors and may contain any
/// number of strictly ordered interior anchors.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct WarpMarkers(Vec<WarpMarker>);

impl<'de> Deserialize<'de> for WarpMarkers {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let mut markers = Vec::<WarpMarker>::deserialize(deserializer)?;
        markers.sort_by_key(|marker| marker.source_frame);
        markers.dedup_by_key(|marker| marker.source_frame);
        markers = markers
            .into_iter()
            .scan(None, |previous_timeline, marker| {
                let keep =
                    previous_timeline.is_none_or(|previous| marker.timeline_frame > previous);
                keep.then(|| {
                    *previous_timeline = Some(marker.timeline_frame);
                    marker
                })
            })
            .collect();
        Ok(Self(markers))
    }
}

impl WarpMarkers {
    pub fn as_slice(&self) -> &[WarpMarker] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn is_neutral(value: &Self) -> bool {
        value.is_empty()
    }

    pub fn source_end(&self, identity_end: u64) -> u64 {
        self.0
            .last()
            .map_or(identity_end, |marker| marker.source_frame)
    }

    pub fn timeline_end(&self, identity_end: u64) -> u64 {
        self.0
            .last()
            .map_or(identity_end, |marker| marker.timeline_frame)
    }

    pub fn interior(&self) -> &[WarpMarker] {
        self.0.get(1..self.0.len().saturating_sub(1)).unwrap_or(&[])
    }

    pub fn add(
        &mut self,
        source_frame: u64,
        timeline_frame: u64,
        source_start: u64,
        source_end: u64,
        timeline_end: u64,
    ) -> bool {
        if source_start >= source_end
            || timeline_end == 0
            || source_frame <= source_start
            || source_frame >= source_end
            || timeline_frame == 0
            || timeline_frame >= timeline_end
        {
            return false;
        }
        self.ensure_boundaries(source_start, source_end, timeline_end);
        let index = self
            .0
            .partition_point(|marker| marker.source_frame < source_frame);
        if self
            .0
            .get(index)
            .is_some_and(|marker| marker.source_frame == source_frame)
            || index == 0
            || index >= self.0.len()
        {
            return false;
        }
        let previous = self.0[index - 1];
        let next = self.0[index];
        if timeline_frame <= previous.timeline_frame || timeline_frame >= next.timeline_frame {
            return false;
        }
        self.0
            .insert(index, WarpMarker::new(source_frame, timeline_frame));
        true
    }

    /// Move an interior marker without allowing it to cross either neighbour.
    /// Returns the clamped timeline frame when the map changed.
    pub fn move_timeline(&mut self, source_frame: u64, timeline_frame: u64) -> Option<u64> {
        let index = self
            .0
            .binary_search_by_key(&source_frame, |marker| marker.source_frame)
            .ok()?;
        if index == 0 || index + 1 >= self.0.len() {
            return None;
        }
        let minimum = self.0[index - 1].timeline_frame.saturating_add(1);
        let maximum = self.0[index + 1].timeline_frame.saturating_sub(1);
        let timeline_frame = timeline_frame.clamp(minimum, maximum);
        if self.0[index].timeline_frame == timeline_frame {
            return None;
        }
        self.0[index].timeline_frame = timeline_frame;
        Some(timeline_frame)
    }

    pub fn remove(&mut self, source_frame: u64) -> bool {
        let Some(index) = self
            .0
            .iter()
            .position(|marker| marker.source_frame == source_frame)
        else {
            return false;
        };
        if index == 0 || index + 1 >= self.0.len() {
            return false;
        }
        self.0.remove(index);
        self.collapse_identity();
        true
    }

    pub fn clear(&mut self) -> bool {
        if self.0.is_empty() {
            return false;
        }
        self.0.clear();
        true
    }

    pub fn source_at_timeline(
        &self,
        timeline_frame: f64,
        source_start: u64,
        timeline_end: u64,
    ) -> f64 {
        if self.0.len() < 2 {
            return source_start as f64 + timeline_frame.clamp(0.0, timeline_end as f64);
        }
        let timeline_frame =
            timeline_frame.clamp(0.0, self.0.last().unwrap().timeline_frame as f64);
        let index = self
            .0
            .partition_point(|marker| marker.timeline_frame as f64 <= timeline_frame)
            .clamp(1, self.0.len() - 1);
        interpolate_by_timeline(self.0[index - 1], self.0[index], timeline_frame)
    }

    pub fn timeline_at_source(&self, source_frame: f64, source_start: u64, source_end: u64) -> f64 {
        if self.0.len() < 2 {
            return (source_frame - source_start as f64)
                .clamp(0.0, source_end.saturating_sub(source_start) as f64);
        }
        let source_frame = source_frame.clamp(source_start as f64, source_end as f64);
        let index = self
            .0
            .partition_point(|marker| marker.source_frame as f64 <= source_frame)
            .clamp(1, self.0.len() - 1);
        interpolate_by_source(self.0[index - 1], self.0[index], source_frame)
    }

    /// Reject a persisted map unless its fixed boundaries and every interior
    /// segment fit the currently materialized Clip audio and timeline.
    pub fn sanitize_for_clip(
        &mut self,
        source_start: u64,
        audio_end: u64,
        timeline_end: u64,
    ) -> bool {
        if self.0.is_empty() {
            return false;
        }
        let valid = self.0.len() >= 2
            && self.0[0] == WarpMarker::new(source_start, 0)
            && self.0.last().is_some_and(|last| {
                last.source_frame <= audio_end && last.timeline_frame == timeline_end
            })
            && self.0.windows(2).all(|pair| {
                pair[0].source_frame < pair[1].source_frame
                    && pair[0].timeline_frame < pair[1].timeline_frame
            });
        if !valid {
            self.0.clear();
        } else {
            self.collapse_identity();
        }
        !valid
    }

    pub fn scale_frames(
        &mut self,
        source_ratio: f64,
        timeline_ratio: f64,
        source_limit: u64,
        timeline_end: u64,
    ) {
        if self.0.is_empty() {
            return;
        }
        for marker in &mut self.0 {
            marker.source_frame =
                ((marker.source_frame as f64 * source_ratio).round() as u64).min(source_limit);
            marker.timeline_frame = (marker.timeline_frame as f64 * timeline_ratio).round() as u64;
        }
        if let Some(last) = self.0.last_mut() {
            last.timeline_frame = timeline_end;
        }
        self.remove_non_monotonic_interiors();
        self.collapse_identity();
    }

    /// Extract a local interval while retaining the exact piecewise mapping.
    pub fn for_fragment(
        &self,
        timeline_start: u64,
        fragment_duration: u64,
        source_start: u64,
        timeline_end: u64,
    ) -> (u64, Self) {
        // Identity clips may loop for much longer than their source window.
        // Their existing ClipTimeline carries that repetition, so introducing
        // a bounded two-point map here would incorrectly stretch the final
        // source segment across the whole fragment.
        if self.0.is_empty() {
            return (
                source_start.saturating_add(timeline_start.min(timeline_end)),
                Self::default(),
            );
        }
        let fragment_end = timeline_start
            .saturating_add(fragment_duration)
            .min(timeline_end);
        let mapped_start = self
            .source_at_timeline(timeline_start as f64, source_start, timeline_end)
            .round() as u64;
        let mapped_end = self
            .source_at_timeline(fragment_end as f64, source_start, timeline_end)
            .round() as u64;
        if fragment_duration == 0 || mapped_end <= mapped_start {
            return (mapped_start, Self::default());
        }
        let mut markers = vec![WarpMarker::new(mapped_start, 0)];
        markers.extend(
            self.interior()
                .iter()
                .copied()
                .filter(|marker| {
                    marker.timeline_frame > timeline_start && marker.timeline_frame < fragment_end
                })
                .map(|marker| {
                    WarpMarker::new(marker.source_frame, marker.timeline_frame - timeline_start)
                }),
        );
        markers.push(WarpMarker::new(mapped_end, fragment_duration));
        let mut fragment = Self(markers);
        fragment.collapse_identity();
        (mapped_start, fragment)
    }

    pub fn resized_timeline(
        &self,
        old_duration: u64,
        new_duration: u64,
        source_start: u64,
        audio_end: u64,
    ) -> Self {
        if self.0.is_empty() || old_duration == new_duration {
            return self.clone();
        }
        if new_duration < old_duration {
            return self
                .for_fragment(0, new_duration, source_start, old_duration)
                .1;
        }

        let mut resized = self.clone();
        let extension = new_duration - old_duration;
        if let Some(last) = resized.0.last_mut() {
            last.source_frame = last.source_frame.saturating_add(extension).min(audio_end);
            last.timeline_frame = new_duration;
        }
        resized.remove_non_monotonic_interiors();
        resized.collapse_identity();
        resized
    }

    fn ensure_boundaries(&mut self, source_start: u64, source_end: u64, timeline_end: u64) {
        if self.0.is_empty() {
            self.0 = vec![
                WarpMarker::new(source_start, 0),
                WarpMarker::new(source_end, timeline_end),
            ];
        }
    }

    fn remove_non_monotonic_interiors(&mut self) {
        if self.0.len() < 2 {
            self.0.clear();
            return;
        }
        let last = *self.0.last().unwrap();
        let mut previous = self.0[0];
        self.0.retain(|marker| {
            let boundary = *marker == previous || *marker == last;
            let keep = boundary
                || (marker.source_frame > previous.source_frame
                    && marker.source_frame < last.source_frame
                    && marker.timeline_frame > previous.timeline_frame
                    && marker.timeline_frame < last.timeline_frame);
            if keep {
                previous = *marker;
            }
            keep
        });
    }

    fn collapse_identity(&mut self) {
        if self.0.len() == 2 {
            let start = self.0[0];
            let end = self.0[1];
            if start.timeline_frame == 0
                && end.source_frame.saturating_sub(start.source_frame) == end.timeline_frame
            {
                self.0.clear();
            }
        }
    }
}

fn interpolate_by_timeline(start: WarpMarker, end: WarpMarker, timeline_frame: f64) -> f64 {
    let timeline_delta = timeline_frame - start.timeline_frame as f64;
    let source_span = end.source_frame.saturating_sub(start.source_frame) as f64;
    let timeline_span = end
        .timeline_frame
        .saturating_sub(start.timeline_frame)
        .max(1) as f64;
    start.source_frame as f64 + timeline_delta * source_span / timeline_span
}

fn interpolate_by_source(start: WarpMarker, end: WarpMarker, source_frame: f64) -> f64 {
    let source_delta = source_frame - start.source_frame as f64;
    let timeline_span = end.timeline_frame.saturating_sub(start.timeline_frame) as f64;
    let source_span = end.source_frame.saturating_sub(start.source_frame).max(1) as f64;
    start.timeline_frame as f64 + source_delta * timeline_span / source_span
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_move_and_remove_preserve_strict_order_and_fixed_boundaries() {
        let mut markers = WarpMarkers::default();
        assert!(markers.add(250, 250, 0, 1_000, 1_000));
        assert!(markers.add(750, 750, 0, 1_000, 1_000));
        assert_eq!(markers.as_slice().len(), 4);
        assert_eq!(markers.move_timeline(250, 900), Some(749));
        assert_eq!(markers.move_timeline(750, 100), None);
        assert!(!markers.remove(0));
        assert!(!markers.remove(1_000));
        assert!(markers.remove(250));
    }

    #[test]
    fn piecewise_mapping_is_reversible() {
        let mut markers = WarpMarkers::default();
        assert!(markers.add(250, 500, 0, 1_000, 1_000));

        assert_eq!(markers.source_at_timeline(250.0, 0, 1_000), 125.0);
        assert_eq!(markers.source_at_timeline(750.0, 0, 1_000), 625.0);
        assert_eq!(markers.timeline_at_source(125.0, 0, 1_000), 250.0);
        assert_eq!(markers.timeline_at_source(625.0, 0, 1_000), 750.0);
    }

    #[test]
    fn timeline_frames_past_the_map_hold_the_final_source_frame() {
        let mut markers = WarpMarkers::default();
        assert!(markers.add(250, 500, 0, 1_000, 1_000));

        assert_eq!(markers.source_at_timeline(1_250.0, 0, 1_000), 1_000.0);
    }

    #[test]
    fn fragment_keeps_segment_timing_even_without_an_interior_marker() {
        let mut markers = WarpMarkers::default();
        assert!(markers.add(250, 500, 0, 1_000, 1_000));

        let (source_start, fragment) = markers.for_fragment(0, 250, 0, 1_000);
        assert_eq!(source_start, 0);
        assert_eq!(
            fragment.as_slice(),
            &[WarpMarker::new(0, 0), WarpMarker::new(125, 250)]
        );
        assert_eq!(fragment.source_at_timeline(125.0, 0, 250), 62.5);
    }

    #[test]
    fn identity_fragment_leaves_repetition_to_the_clip_timeline() {
        let markers = WarpMarkers::default();

        let (source_start, fragment) = markers.for_fragment(50, 150, 0, 100);

        assert_eq!(source_start, 50);
        assert!(fragment.is_empty());
    }

    #[test]
    fn malformed_persisted_maps_are_cleared_on_clip_sanitization() {
        let mut markers: WarpMarkers = serde_json::from_str(
            r#"[
                {"source_frame":0,"timeline_frame":0},
                {"source_frame":500,"timeline_frame":700},
                {"source_frame":1000,"timeline_frame":600}
            ]"#,
        )
        .unwrap();
        assert!(markers.sanitize_for_clip(0, 1_000, 1_000));
        assert!(markers.is_empty());
    }
}
