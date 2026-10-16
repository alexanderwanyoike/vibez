//! Source-frame random access keeps callback partitions and observation drains out of dither.

// SplitMix64 output permutation: https://prng.di.unimi.it/splitmix64.c
// Pinned source SHA256: 071795a8e29978a5cbd7015ce8f7d772e7ab4631e574e9102b748fe99105ff3d.
/*  Written in 2015 by Sebastiano Vigna (vigna@acm.org)

To the extent possible under law, the author has dedicated all copyright
and related and neighboring rights to this software to the public domain
worldwide.

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR
IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE. */

const GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;
const IDENTITY_DOMAIN: u64 = 0xd1b5_4a32_d192_ed03;

pub(super) struct Random {
    streams: [u64; 5],
}

impl Random {
    pub(super) fn for_identity(identity: u64) -> Self {
        let base = scramble(identity ^ IDENTITY_DOMAIN);
        Self {
            streams: std::array::from_fn(|tag| {
                scramble(base.wrapping_add(GAMMA.wrapping_mul(tag as u64 + 1)))
            }),
        }
    }

    fn uniform(&self, frame: u64, tag: usize) -> f64 {
        let word = scramble(self.streams[tag].wrapping_add(GAMMA.wrapping_mul(frame)));
        // f32 would round the largest midpoint to1.0 and lose the open interval.
        ((word >> 40) as f64 + 0.5) / 16_777_216.0
    }

    pub(super) fn tpdf(&self, frame: u64, channel: usize) -> f64 {
        let first = channel * 2 + 1;
        self.uniform(frame, first) - self.uniform(frame, first + 1)
    }

    pub(super) fn clock(&self, frame: u64) -> f64 {
        2.0 * self.uniform(frame, 0) - 1.0
    }
}

fn scramble(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}
