use serde::{Deserialize, Serialize};

/// Opt-in playback grid for non-destructive MIDI clip Swing.
///
/// `Off` deliberately remains the default: Vibez never guesses the rhythmic
/// intent of freely recorded notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrooveGrid {
    #[default]
    Off,
    Eighth,
    Sixteenth,
}

impl GrooveGrid {
    pub const ALL: [Self; 3] = [Self::Off, Self::Eighth, Self::Sixteenth];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Eighth => "1/8",
            Self::Sixteenth => "1/16",
        }
    }

    /// Map one clip-local beat through the MPC2000XL pair shape.
    ///
    /// Pair endpoints stay fixed and the straight midpoint moves to the
    /// nearest 96 PPQN Swing tick. Interpolating either side maps human timing
    /// without a tolerance window and cannot reorder events.
    pub fn map_beat(self, beat: f64, swing: SwingAmount) -> f64 {
        let pair_ticks = match self {
            Self::Off => return beat,
            Self::Eighth => GrooveProfile::MPC2000XL_PPQN,
            Self::Sixteenth => GrooveProfile::MPC2000XL_PPQN / 2,
        };
        let pair_beats = pair_ticks as f64 / GrooveProfile::MPC2000XL_PPQN as f64;
        let pair_start = (beat / pair_beats).floor() * pair_beats;
        let phase = beat - pair_start;
        let midpoint = pair_beats / 2.0;
        let swung_ticks = (pair_ticks as f64 * swing.get() as f64).round();
        let swung_midpoint = swung_ticks / GrooveProfile::MPC2000XL_PPQN as f64;

        if phase <= midpoint {
            pair_start + phase / midpoint * swung_midpoint
        } else {
            pair_start
                + swung_midpoint
                + (phase - midpoint) / midpoint * (pair_beats - swung_midpoint)
        }
    }

    pub const fn pair_beats(self) -> Option<f64> {
        match self {
            Self::Off => None,
            Self::Eighth => Some(1.0),
            Self::Sixteenth => Some(0.5),
        }
    }
}

impl std::fmt::Display for GrooveGrid {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.label())
    }
}

/// Immutable timing-model identity used to interpret Project Swing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GrooveProfile {
    #[default]
    #[serde(rename = "mpc_2000xl_v1")]
    Mpc2000XlV1,
}

impl GrooveProfile {
    pub const MPC2000XL_PPQN: u32 = 96;

    pub const fn label(self) -> &'static str {
        match self {
            Self::Mpc2000XlV1 => "MPC2000XL",
        }
    }

    pub const fn swings(self, rate: NoteRepeatRate) -> bool {
        match self {
            Self::Mpc2000XlV1 => {
                matches!(rate, NoteRepeatRate::Eighth | NoteRepeatRate::Sixteenth)
            }
        }
    }
}

/// Project-wide MPC2000XL Swing ratio. `0.50` is straight, approximately
/// `0.66` is triplet feel, and `0.75` produces a 3:1 long/short pair.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(transparent)]
pub struct SwingAmount(f32);

impl SwingAmount {
    pub const MIN: f32 = 0.50;
    pub const MAX: f32 = 0.75;
    pub const STRAIGHT: Self = Self(Self::MIN);

    pub fn new(value: f32) -> Self {
        Self(value.clamp(Self::MIN, Self::MAX))
    }

    pub const fn get(self) -> f32 {
        self.0
    }

    pub fn effective(self, offset: Option<SwingOffset>) -> Self {
        Self::new(self.0 + offset.unwrap_or_default().get())
    }
}

impl Default for SwingAmount {
    fn default() -> Self {
        Self::STRAIGHT
    }
}

impl<'de> Deserialize<'de> for SwingAmount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = f32::deserialize(deserializer)?;
        if value.is_finite() {
            Ok(Self::new(value))
        } else {
            Err(serde::de::Error::custom("Swing must be a finite number"))
        }
    }
}

/// Optional percentage-point adjustment combined with Project Swing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(transparent)]
pub struct SwingOffset(f32);

impl SwingOffset {
    pub const MIN: f32 = -0.25;
    pub const MAX: f32 = 0.25;

    pub fn new(value: f32) -> Self {
        Self(value.clamp(Self::MIN, Self::MAX))
    }

    pub const fn get(self) -> f32 {
        self.0
    }

    pub fn from_normalized(value: f32) -> Self {
        Self::new((value.clamp(0.0, 1.0) - 0.5) * (Self::MAX - Self::MIN))
    }

    pub fn normalized(self) -> f32 {
        (self.0 - Self::MIN) / (Self::MAX - Self::MIN)
    }
}

impl Default for SwingOffset {
    fn default() -> Self {
        Self(0.0)
    }
}

impl<'de> Deserialize<'de> for SwingOffset {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = f32::deserialize(deserializer)?;
        if value.is_finite() {
            Ok(Self::new(value))
        } else {
            Err(serde::de::Error::custom(
                "Project Track Swing offset must be a finite number",
            ))
        }
    }
}

/// Tempo-synced Instrument Note Repeat subdivisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteRepeatRate {
    Quarter,
    QuarterTriplet,
    Eighth,
    EighthTriplet,
    #[default]
    Sixteenth,
    SixteenthTriplet,
    ThirtySecond,
    ThirtySecondTriplet,
}

impl NoteRepeatRate {
    pub const ALL: [Self; 8] = [
        Self::Quarter,
        Self::QuarterTriplet,
        Self::Eighth,
        Self::EighthTriplet,
        Self::Sixteenth,
        Self::SixteenthTriplet,
        Self::ThirtySecond,
        Self::ThirtySecondTriplet,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Quarter => "1/4",
            Self::QuarterTriplet => "1/4T",
            Self::Eighth => "1/8",
            Self::EighthTriplet => "1/8T",
            Self::Sixteenth => "1/16",
            Self::SixteenthTriplet => "1/16T",
            Self::ThirtySecond => "1/32",
            Self::ThirtySecondTriplet => "1/32T",
        }
    }

    pub const fn straight_beats(self) -> f64 {
        match self {
            Self::Quarter | Self::QuarterTriplet => 1.0,
            Self::Eighth | Self::EighthTriplet => 0.5,
            Self::Sixteenth | Self::SixteenthTriplet => 0.25,
            Self::ThirtySecond | Self::ThirtySecondTriplet => 0.125,
        }
    }

    pub const fn is_triplet(self) -> bool {
        matches!(
            self,
            Self::QuarterTriplet
                | Self::EighthTriplet
                | Self::SixteenthTriplet
                | Self::ThirtySecondTriplet
        )
    }

    pub const fn interval_beats(self) -> f64 {
        if self.is_triplet() {
            self.straight_beats() * (2.0 / 3.0)
        } else {
            self.straight_beats()
        }
    }
}

impl std::fmt::Display for NoteRepeatRate {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.label())
    }
}

#[cfg(test)]
mod groove_tests {
    use super::*;

    #[test]
    fn groove_grid_maps_the_midpoint_to_the_96_ppqn_swing_tick() {
        assert!(
            (GrooveGrid::Sixteenth.map_beat(0.25, SwingAmount::new(0.66)) - 32.0 / 96.0).abs()
                < 1e-9
        );
        assert!(
            (GrooveGrid::Eighth.map_beat(0.5, SwingAmount::new(0.56)) - 54.0 / 96.0).abs() < 1e-9
        );
    }

    #[test]
    fn groove_grid_maps_human_timing_monotonically_without_a_tolerance() {
        let swing = SwingAmount::new(0.66);
        let mapped =
            [0.0, 0.125, 0.25, 0.375, 0.5].map(|beat| GrooveGrid::Sixteenth.map_beat(beat, swing));
        assert_eq!(mapped[0], 0.0);
        assert!((mapped[1] - 16.0 / 96.0).abs() < 1e-9);
        assert!((mapped[2] - 32.0 / 96.0).abs() < 1e-9);
        assert!((mapped[3] - 40.0 / 96.0).abs() < 1e-9);
        assert_eq!(mapped[4], 0.5);
        assert!(mapped.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn groove_grid_off_is_an_identity_map() {
        assert_eq!(
            GrooveGrid::Off.map_beat(0.371, SwingAmount::new(0.75)),
            0.371
        );
    }

    #[test]
    fn mpc2000xl_profile_has_a_stable_persisted_identity() {
        assert_eq!(
            serde_json::to_string(&GrooveProfile::Mpc2000XlV1).unwrap(),
            "\"mpc_2000xl_v1\""
        );
        assert_eq!(GrooveProfile::default(), GrooveProfile::Mpc2000XlV1);
    }

    #[test]
    fn mpc2000xl_swing_and_track_offsets_use_percentage_point_semantics() {
        assert_eq!(SwingAmount::new(0.0), SwingAmount::STRAIGHT);
        assert_eq!(SwingAmount::new(1.0).get(), 0.75);
        assert_eq!(SwingAmount::new(0.56).get(), 0.56);
        assert_eq!(
            SwingAmount::new(0.56)
                .effective(Some(SwingOffset::new(0.04)))
                .get(),
            0.60
        );
        assert_eq!(
            SwingAmount::new(0.70)
                .effective(Some(SwingOffset::new(0.20)))
                .get(),
            0.75
        );
        assert!((SwingOffset::new(0.04).normalized() - 0.58).abs() < f32::EPSILON);
        assert!((SwingOffset::from_normalized(0.58).get() - 0.04).abs() < f32::EPSILON);
    }
}

/// Musical boundary at which a resident Section launch becomes effective.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionLaunchQuantization {
    Immediate,
    OneBeat,
    #[default]
    OneBar,
    EndOfSection,
}

impl SectionLaunchQuantization {
    pub const ALL: [Self; 4] = [
        Self::Immediate,
        Self::OneBeat,
        Self::OneBar,
        Self::EndOfSection,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Immediate => "Immediate",
            Self::OneBeat => "1 Beat",
            Self::OneBar => "1 Bar",
            Self::EndOfSection => "End of Section",
        }
    }
}

impl std::fmt::Display for SectionLaunchQuantization {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.label())
    }
}
