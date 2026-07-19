use serde::{Deserialize, Serialize};

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
