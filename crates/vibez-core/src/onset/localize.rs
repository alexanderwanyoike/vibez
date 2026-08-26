use crate::audio_buffer::DecodedAudio;

use super::frames_for_ms;

// At 44.1 kHz these are aubio's 1,024-sample analysis window and
// librosa-style 128-sample localisation hop, expressed in time so the
// behaviour stays consistent at other sample rates.
const BACKTRACK_WINDOW_MS: f32 = 23.22;
const BACKTRACK_HOP_MS: f32 = 2.90;
const BACKTRACK_MAX_MS: f32 = 50.0;

pub(super) fn canonicalize_onsets(mut onsets: Vec<u64>, minimum_interval_frames: u64) -> Vec<u64> {
    onsets.sort_unstable();
    let mut canonical = Vec::with_capacity(onsets.len());
    for onset in onsets {
        if canonical
            .last()
            .is_none_or(|previous: &u64| onset.abs_diff(*previous) > minimum_interval_frames)
        {
            canonical.push(onset);
        }
    }
    canonical
}

/// Localise each `(estimated onset, detected peak)` pair at a preceding energy
/// minimum. If no suitable minimum exists, retain aubio's delay-compensated
/// estimate rather than the later spectral peak.
pub(super) fn backtrack_to_energy_minima(audio: &DecodedAudio, events: &[(u64, u64)]) -> Vec<u64> {
    if events.is_empty() || audio.num_frames() < 3 {
        return events.iter().map(|(estimated, _)| *estimated).collect();
    }

    let hop = frames_for_ms(audio.sample_rate, BACKTRACK_HOP_MS).max(1);
    let window = frames_for_ms(audio.sample_rate, BACKTRACK_WINDOW_MS).max(1);
    let energy = rms_energy_envelope(audio, window, hop);
    backtrack_events_to_minima(
        events,
        &energy,
        hop,
        frames_for_ms(audio.sample_rate, BACKTRACK_MAX_MS),
    )
}

/// Build a trailing RMS envelope with memory bounded by the analysis window.
/// The old prefix-sum implementation allocated one `f64` per source frame,
/// which could consume roughly 100 MB for a long stereo take.
fn rms_energy_envelope(audio: &DecodedAudio, window: usize, hop: usize) -> Vec<f32> {
    let frames = audio.num_frames();
    if frames == 0 || window == 0 || hop == 0 {
        return Vec::new();
    }

    let channel_count = audio.channels.len().max(1) as f64;
    let mut rolling = vec![0.0f64; window];
    let mut rolling_sum = 0.0f64;
    let mut cursor = 0usize;
    let mut envelope = Vec::with_capacity(frames.div_ceil(hop));

    for end in (0..frames).step_by(hop) {
        while cursor < end {
            let square_sum = audio
                .channels
                .iter()
                .map(|channel| f64::from(channel.get(cursor).copied().unwrap_or(0.0)).powi(2))
                .sum::<f64>()
                / channel_count;
            let slot = cursor % window;
            rolling_sum += square_sum - rolling[slot];
            rolling[slot] = square_sum;
            cursor += 1;
        }
        let frame_count = end.min(window).max(1) as f64;
        envelope.push((rolling_sum / frame_count).sqrt() as f32);
    }
    envelope
}

// The local-minimum matching rule is derived from librosa 0.11.0's
// `onset_backtrack`, copyright (c) 2013-2023 the librosa development team,
// distributed under the ISC licence:
//
// Permission to use, copy, modify, and/or distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
// WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
// MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY
// SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
// WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION
// OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN
// CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
//
// Vibez adds a bounded source-frame look-back because a DAW marker must not
// jump into an earlier event.
pub(super) fn backtrack_events_to_minima(
    events: &[(u64, u64)],
    energy: &[f32],
    hop: usize,
    max_lookback: usize,
) -> Vec<u64> {
    if energy.len() < 3 || hop == 0 {
        return events.iter().map(|(estimated, _)| *estimated).collect();
    }

    let mut minima = vec![0usize];
    minima.extend(energy.windows(3).enumerate().filter_map(|(index, values)| {
        (values[1] <= values[0] && values[1] < values[2]).then_some(index + 1)
    }));

    events
        .iter()
        .map(|&(estimated, detected_peak)| {
            let peak = usize::try_from(detected_peak).unwrap_or(usize::MAX);
            let first_allowed = peak.saturating_sub(max_lookback);
            let after_peak = minima.partition_point(|minimum| minimum.saturating_mul(hop) <= peak);
            minima[..after_peak]
                .iter()
                .rev()
                .map(|minimum| minimum.saturating_mul(hop))
                .find(|minimum| *minimum >= first_allowed)
                .map_or(estimated, |minimum| minimum as u64)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolling_rms_matches_trailing_window_values() {
        let audio = DecodedAudio {
            channels: vec![vec![1.0, 2.0, 3.0, 4.0, 5.0]],
            sample_rate: 44_100,
        };
        let actual = rms_energy_envelope(&audio, 3, 1);
        let expected = [
            0.0,
            1.0,
            (2.5f32).sqrt(),
            (14.0f32 / 3.0).sqrt(),
            (29.0f32 / 3.0).sqrt(),
        ];
        for (actual, expected) in actual.iter().zip(expected) {
            assert!((actual - expected).abs() < 1.0e-6, "{actual} != {expected}");
        }
    }

    #[test]
    fn missing_minimum_keeps_delay_compensated_estimate() {
        let energy = [1.0, 2.0, 3.0, 4.0];
        assert_eq!(
            backtrack_events_to_minima(&[(200, 300)], &energy, 128, 64),
            [200]
        );
    }
}
