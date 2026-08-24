//! Canonical source-frame Transient Markers for Audio Clips.

use serde::{Deserialize, Serialize};

/// How a Transient Marker entered the Clip. Detected markers remain visual
/// suggestions until a producer moves them or adds a marker by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransientMarkerKind {
    Suggested,
    Authored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransientMarker {
    source_frame: u64,
    kind: TransientMarkerKind,
}

impl TransientMarker {
    pub const fn new(source_frame: u64, kind: TransientMarkerKind) -> Self {
        Self { source_frame, kind }
    }

    pub const fn source_frame(self) -> u64 {
        self.source_frame
    }

    pub const fn kind(self) -> TransientMarkerKind {
        self.kind
    }
}

/// Ordered, unique Transient Markers for one Audio Clip.
///
/// Construction and deserialization always canonicalize positions. When a
/// detected suggestion collides with an authored marker, the authored marker
/// wins.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct TransientMarkers(Vec<TransientMarker>);

impl<'de> Deserialize<'de> for TransientMarkers {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let markers = Vec::<TransientMarker>::deserialize(deserializer)?;
        Ok(Self::canonical(markers))
    }
}

impl TransientMarkers {
    fn canonical(mut markers: Vec<TransientMarker>) -> Self {
        markers.sort_by_key(|marker| {
            (
                marker.source_frame,
                marker.kind == TransientMarkerKind::Suggested,
            )
        });
        markers.dedup_by_key(|marker| marker.source_frame);
        Self(markers)
    }

    pub fn as_slice(&self) -> &[TransientMarker] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn contains(&self, source_frame: u64) -> bool {
        self.0
            .binary_search_by_key(&source_frame, |marker| marker.source_frame)
            .is_ok()
    }

    pub fn replace_suggestions(&mut self, frames: impl IntoIterator<Item = u64>) {
        let mut markers: Vec<_> = self
            .0
            .iter()
            .copied()
            .filter(|marker| marker.kind == TransientMarkerKind::Authored)
            .collect();
        markers.extend(
            frames
                .into_iter()
                .map(|frame| TransientMarker::new(frame, TransientMarkerKind::Suggested)),
        );
        *self = Self::canonical(markers);
    }

    pub fn add_authored(&mut self, source_frame: u64) -> bool {
        if self
            .0
            .iter()
            .any(|marker| marker.source_frame == source_frame)
        {
            return false;
        }
        self.0.push(TransientMarker::new(
            source_frame,
            TransientMarkerKind::Authored,
        ));
        self.0.sort_by_key(|marker| marker.source_frame);
        true
    }

    pub fn move_and_author(&mut self, from: u64, to: u64) -> bool {
        if from == to || !self.0.iter().any(|marker| marker.source_frame == from) {
            return false;
        }
        self.0
            .retain(|marker| marker.source_frame != from && marker.source_frame != to);
        self.0
            .push(TransientMarker::new(to, TransientMarkerKind::Authored));
        self.0.sort_by_key(|marker| marker.source_frame);
        true
    }

    pub fn remove(&mut self, source_frame: u64) -> bool {
        let before = self.0.len();
        self.0.retain(|marker| marker.source_frame != source_frame);
        self.0.len() != before
    }

    pub fn retain_source_range(&mut self, start: u64, end: u64) {
        self.0
            .retain(|marker| marker.source_frame >= start && marker.source_frame <= end);
    }

    pub fn scale_source_frames(&mut self, ratio: f64, source_end: u64) {
        let markers = self.0.iter().map(|marker| {
            TransientMarker::new(
                ((marker.source_frame as f64 * ratio).round() as u64).min(source_end),
                marker.kind,
            )
        });
        *self = Self::canonical(markers.collect());
    }

    pub fn is_neutral(value: &Self) -> bool {
        value.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_canonicalizes_and_preserves_authored_collisions() {
        let json = r#"[
            {"source_frame":200,"kind":"suggested"},
            {"source_frame":100,"kind":"suggested"},
            {"source_frame":200,"kind":"authored"},
            {"source_frame":100,"kind":"suggested"}
        ]"#;
        let markers: TransientMarkers = serde_json::from_str(json).unwrap();

        assert_eq!(
            markers.as_slice(),
            &[
                TransientMarker::new(100, TransientMarkerKind::Suggested),
                TransientMarker::new(200, TransientMarkerKind::Authored),
            ]
        );
    }

    #[test]
    fn detecting_replaces_only_suggestions_and_moving_authors_a_marker() {
        let mut markers = TransientMarkers::default();
        assert!(markers.add_authored(200));
        markers.replace_suggestions([300, 100, 200]);
        assert_eq!(
            markers.as_slice(),
            &[
                TransientMarker::new(100, TransientMarkerKind::Suggested),
                TransientMarker::new(200, TransientMarkerKind::Authored),
                TransientMarker::new(300, TransientMarkerKind::Suggested),
            ]
        );

        assert!(markers.move_and_author(100, 250));
        assert_eq!(
            markers.as_slice(),
            &[
                TransientMarker::new(200, TransientMarkerKind::Authored),
                TransientMarker::new(250, TransientMarkerKind::Authored),
                TransientMarker::new(300, TransientMarkerKind::Suggested),
            ]
        );
        assert!(markers.remove(300));
    }
}
