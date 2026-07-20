use serde::{Deserialize, Serialize};

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
