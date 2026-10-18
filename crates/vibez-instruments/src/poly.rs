//! Which voice a new note takes in a polyphonic instrument.

/// What the choice needs to know of a voice.
pub trait PolyVoice {
    fn pitch(&self) -> u8;
    /// The note count when the voice started; smallest is oldest.
    fn age(&self) -> u64;
    fn active(&self) -> bool;
    fn released(&self) -> bool;
}

/// The voice a new `pitch` takes: its own when it is already sounding, so a
/// repeated note takes over rather than stacking, else a free one, else the
/// oldest released, else the oldest.
pub fn choose<V: PolyVoice>(voices: &[V], pitch: u8) -> usize {
    let all = 0..voices.len();
    let oldest = |candidates: &mut dyn Iterator<Item = usize>| {
        candidates.min_by_key(|&index| voices[index].age())
    };
    all.clone()
        .find(|&index| voices[index].active() && voices[index].pitch() == pitch)
        .or_else(|| all.clone().find(|&index| !voices[index].active()))
        .or_else(|| oldest(&mut all.clone().filter(|&index| voices[index].released())))
        .or_else(|| oldest(&mut all.clone()))
        .unwrap_or(0)
}
