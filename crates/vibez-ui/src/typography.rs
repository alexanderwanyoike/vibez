//! Bundled type roles used where the system fallback cannot express the
//! intended interface hierarchy consistently across platforms.

use iced::{font::Weight, Font};

pub const PLEX_SANS_CONDENSED_MEDIUM_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/ibm-plex/IBMPlexSansCondensed-Medium.ttf");
pub const PLEX_SANS_CONDENSED_SEMIBOLD_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/ibm-plex/IBMPlexSansCondensed-SemiBold.ttf");
pub const PLEX_MONO_MEDIUM_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/ibm-plex/IBMPlexMono-Medium.ttf");
pub const PLEX_MONO_SEMIBOLD_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/ibm-plex/IBMPlexMono-SemiBold.ttf");

pub const PERFORM_DISPLAY: Font = Font {
    weight: Weight::Medium,
    ..Font::with_name("IBM Plex Sans Condensed")
};
pub const PERFORM_LABEL: Font = Font {
    weight: Weight::Semibold,
    ..Font::with_name("IBM Plex Sans Condensed")
};
pub const PERFORM_TECH: Font = Font {
    weight: Weight::Medium,
    ..Font::with_name("IBM Plex Mono")
};
pub const PERFORM_TECH_STRONG: Font = Font {
    weight: Weight::Semibold,
    ..Font::with_name("IBM Plex Mono")
};
