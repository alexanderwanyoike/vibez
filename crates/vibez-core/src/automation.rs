//! Automation lanes: parameter values drawn over time.
//!
//! A lane targets one parameter and holds points sorted by beat.
//! Evaluation is linear interpolation between points, holding the
//! first value before the first point and the last value after the
//! last point. Block-rate evaluation (once per render segment) is
//! the v1 contract; no sample-accurate ramps yet.

use serde::{Deserialize, Serialize};

use crate::id::{EffectId, LaneId};

/// One breakpoint on a lane. `value` is in the target parameter's
/// native unit (gain multiplier, pan 0..1, effect param range).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AutomationPoint {
    pub beat: f64,
    pub value: f32,
    /// Shape of the segment LEAVING this point: 0 is linear,
    /// positive bends toward the destination late (ease-in),
    /// negative early (ease-out). Range -1..1.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub curve: f32,
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

/// Apply a segment's curve shaping to linear progress `t` (0..1).
pub fn shape(t: f32, curve: f32) -> f32 {
    if curve == 0.0 {
        return t;
    }
    // Exponent sweep: curve -1 -> t^(1/8) (early), +1 -> t^8 (late).
    let exp = 2.0_f32.powf(curve * 3.0);
    t.powf(exp)
}

/// What a lane drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AutomationTarget {
    TrackGain,
    TrackPan,
    /// Project-relative Track Swing offset, normalized from -25..+25 points.
    #[serde(rename = "track_swing_offset")]
    TrackSwingOffset,
    EffectParam {
        effect_id: EffectId,
        param_index: usize,
    },
    InstrumentParam {
        param_index: usize,
    },
    /// Third-party plugin parameter. `effect_id` is `None` for the
    /// track's plugin instrument.
    PluginParam {
        effect_id: Option<EffectId>,
        param_id: u32,
    },
    /// The track's post-fader send amount into a bus (native 0..1).
    Send {
        bus_id: crate::id::TrackId,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationLane {
    pub id: LaneId,
    pub target: AutomationTarget,
    /// Sorted by beat. Editing code must keep this invariant;
    /// [`AutomationLane::insert_point`] does.
    pub points: Vec<AutomationPoint>,
}

impl AutomationLane {
    pub fn new(target: AutomationTarget) -> Self {
        Self {
            id: LaneId::new(),
            target,
            points: Vec::new(),
        }
    }

    /// Evaluate the lane at `beat`. `None` when the lane is empty.
    pub fn value_at(&self, beat: f64) -> Option<f32> {
        let points = &self.points;
        if points.is_empty() {
            return None;
        }
        if beat <= points[0].beat {
            return Some(points[0].value);
        }
        let last = points[points.len() - 1];
        if beat >= last.beat {
            return Some(last.value);
        }
        // partition_point: first index whose beat is > beat.
        let idx = points.partition_point(|p| p.beat <= beat);
        let a = points[idx - 1];
        let b = points[idx];
        let span = b.beat - a.beat;
        if span <= f64::EPSILON {
            return Some(b.value);
        }
        let t = shape(((beat - a.beat) / span) as f32, a.curve);
        Some(a.value + (b.value - a.value) * t)
    }

    /// Insert keeping beat order. Replaces an existing point on the
    /// same beat (within a hair) instead of stacking duplicates.
    pub fn insert_point(&mut self, point: AutomationPoint) {
        const EPS: f64 = 1e-9;
        match self
            .points
            .iter()
            .position(|p| (p.beat - point.beat).abs() < EPS)
        {
            Some(i) => self.points[i] = point,
            None => {
                let idx = self.points.partition_point(|p| p.beat < point.beat);
                self.points.insert(idx, point);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_swing_offset_has_a_stable_persisted_identity() {
        let json = serde_json::to_string(&AutomationTarget::TrackSwingOffset).unwrap();
        assert_eq!(json, "\"track_swing_offset\"");
        assert_eq!(
            serde_json::from_str::<AutomationTarget>(&json).unwrap(),
            AutomationTarget::TrackSwingOffset
        );
    }

    fn lane(points: &[(f64, f32)]) -> AutomationLane {
        let mut l = AutomationLane::new(AutomationTarget::TrackGain);
        for (beat, value) in points {
            l.insert_point(AutomationPoint {
                beat: *beat,
                value: *value,
                curve: 0.0,
            });
        }
        l
    }

    #[test]
    fn empty_lane_has_no_value() {
        assert_eq!(lane(&[]).value_at(4.0), None);
    }

    #[test]
    fn holds_first_and_last_values_outside_range() {
        let l = lane(&[(4.0, 0.2), (8.0, 0.8)]);
        assert_eq!(l.value_at(0.0), Some(0.2));
        assert_eq!(l.value_at(100.0), Some(0.8));
    }

    #[test]
    fn interpolates_linearly_between_points() {
        let l = lane(&[(0.0, 0.0), (8.0, 1.0)]);
        assert_eq!(l.value_at(4.0), Some(0.5));
        assert_eq!(l.value_at(2.0), Some(0.25));
    }

    #[test]
    fn insert_keeps_order_and_replaces_same_beat() {
        let mut l = lane(&[(8.0, 0.8), (0.0, 0.1), (4.0, 0.4)]);
        let beats: Vec<f64> = l.points.iter().map(|p| p.beat).collect();
        assert_eq!(beats, vec![0.0, 4.0, 8.0]);
        l.insert_point(AutomationPoint {
            beat: 4.0,
            value: 0.9,
            curve: 0.0,
        });
        assert_eq!(l.points.len(), 3);
        assert_eq!(l.value_at(4.0), Some(0.9));
    }

    #[test]
    fn serde_roundtrip() {
        let l = lane(&[(0.0, 0.0), (16.0, 1.0)]);
        let json = serde_json::to_string(&l).unwrap();
        let back: AutomationLane = serde_json::from_str(&json).unwrap();
        assert_eq!(l, back);
    }
}

#[cfg(test)]
mod curve_tests {
    use super::*;

    #[test]
    fn curve_bends_interpolation() {
        let mut l = AutomationLane::new(AutomationTarget::TrackGain);
        l.insert_point(AutomationPoint {
            beat: 0.0,
            value: 0.0,
            curve: 1.0, // strong ease-in: stays low, rises late
        });
        l.insert_point(AutomationPoint {
            beat: 8.0,
            value: 1.0,
            curve: 0.0,
        });
        let mid = l.value_at(4.0).unwrap();
        assert!(mid < 0.05, "eased-in midpoint should hug the start: {mid}");
        // Linear again once the curve resets.
        l.points[0].curve = 0.0;
        assert_eq!(l.value_at(4.0), Some(0.5));
    }

    #[test]
    fn zero_curve_serializes_compactly_and_loads_back() {
        let mut l = AutomationLane::new(AutomationTarget::TrackPan);
        l.insert_point(AutomationPoint {
            beat: 0.0,
            value: 0.5,
            curve: 0.0,
        });
        let json = serde_json::to_string(&l).unwrap();
        assert!(!json.contains("curve"));
        let back: AutomationLane = serde_json::from_str(&json).unwrap();
        assert_eq!(back.points[0].curve, 0.0);
    }
}
