//! Keeps a Clip off the pitches a held live key owns, so a key held while
//! recording sounds until it is released.

use super::groove::TimedNoteOn;

/// No MIDI pitch, marking a dropped event until the lists are compacted.
const DROPPED: u8 = 128;

/// Drops this block's source note-ons for pitches a live key holds, and
/// source note-offs for pitches a live key took over. A Clip being recorded
/// plays back the note still held, and its copy would otherwise restart the
/// pitch or cut the live voice. A kept note-on hands its pitch back to the
/// Clip. Both lists are sorted by frame, and an off at the same frame as an
/// on goes first, as the renderers send them.
pub(super) fn yield_to_live_keys(
    ons: &mut Vec<TimedNoteOn>,
    offs: &mut Vec<(u32, u8)>,
    live_held: u128,
    live_owned: &mut u128,
) {
    let (mut on, mut off) = (0, 0);
    while on < ons.len() || off < offs.len() {
        if off < offs.len() && (on >= ons.len() || offs[off].0 <= ons[on].0) {
            if (live_held | *live_owned) & (1u128 << offs[off].1) != 0 {
                offs[off].1 = DROPPED;
            }
            off += 1;
        } else {
            let bit = 1u128 << ons[on].1;
            if live_held & bit != 0 {
                ons[on].1 = DROPPED;
            } else {
                *live_owned &= !bit;
            }
            on += 1;
        }
    }
    ons.retain(|event| event.1 != DROPPED);
    offs.retain(|event| event.1 != DROPPED);
}
