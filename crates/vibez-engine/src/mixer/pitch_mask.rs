//! Walks the pitches set in a 128-bit sounding-note mask, lowest first.

pub(crate) fn pitches(mut mask: u128) -> impl Iterator<Item = u8> {
    std::iter::from_fn(move || {
        (mask != 0).then(|| {
            let pitch = mask.trailing_zeros() as u8;
            mask &= mask - 1;
            pitch
        })
    })
}
