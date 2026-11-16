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

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_core::id::ClipId;

    const C4: u8 = 60;
    const D4: u8 = 62;
    const C4_BIT: u128 = 1 << C4;

    /// Events as (frame, pitch).
    type Events = Vec<(u32, u8)>;

    /// Runs one block and returns the kept ons and offs as (frame, pitch).
    fn yield_block(
        ons: &[(u32, u8)],
        offs: &[(u32, u8)],
        live_held: u128,
        live_owned: &mut u128,
    ) -> (Events, Events) {
        let clip = ClipId::new();
        let mut ons = ons
            .iter()
            .map(|&(frame, pitch)| (frame, pitch, 100, clip))
            .collect();
        let mut offs = offs.to_vec();
        yield_to_live_keys(&mut ons, &mut offs, live_held, live_owned);
        let ons = ons
            .into_iter()
            .map(|(frame, pitch, _, _)| (frame, pitch))
            .collect();
        (ons, offs)
    }

    #[test]
    fn without_a_live_key_every_event_is_kept() {
        let mut owned = 0;
        let ons = [(0, C4), (10, C4)];
        let offs = [(20, C4), (30, C4), (40, C4)];

        assert_eq!(
            yield_block(&ons, &offs, 0, &mut owned),
            (ons.to_vec(), offs.to_vec())
        );
        assert_eq!(owned, 0);
    }

    #[test]
    fn a_held_key_drops_the_clip_events_of_its_pitch_only() {
        let mut owned = C4_BIT;

        assert_eq!(
            yield_block(
                &[(0, C4), (0, D4)],
                &[(20, C4), (20, D4)],
                C4_BIT,
                &mut owned
            ),
            (vec![(0, D4)], vec![(20, D4)])
        );
        assert_eq!(
            owned, C4_BIT,
            "a dropped note-on leaves the pitch the key's"
        );
    }

    #[test]
    fn a_released_key_keeps_its_pitch_until_a_clip_note_on_reclaims_it() {
        let mut owned = C4_BIT;

        assert_eq!(
            yield_block(&[(10, C4)], &[(0, C4), (20, C4)], 0, &mut owned),
            (vec![(10, C4)], vec![(20, C4)])
        );
        assert_eq!(owned, 0);
    }

    #[test]
    fn an_off_at_the_frame_of_a_reclaiming_on_is_still_the_keys() {
        let mut owned = C4_BIT;

        assert_eq!(
            yield_block(&[(10, C4)], &[(10, C4)], 0, &mut owned),
            (vec![(10, C4)], vec![])
        );
        assert_eq!(owned, 0);
    }
}
