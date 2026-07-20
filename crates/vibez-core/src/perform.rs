use serde::{Deserialize, Serialize};

/// Project-wide Swing amount. `0.0` is straight and `1.0` delays every
/// generated offbeat to a 2:1 triplet feel.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(transparent)]
pub struct SwingAmount(f32);

impl SwingAmount {
    pub const STRAIGHT: Self = Self(0.0);

    pub fn new(value: f32) -> Self {
        Self(value.clamp(0.0, 1.0))
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

/// Optional Project Track adjustment combined with Project Swing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(transparent)]
pub struct SwingOffset(f32);

impl SwingOffset {
    pub fn new(value: f32) -> Self {
        Self(value.clamp(-1.0, 1.0))
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
