//! Waveform extrema of a whole source at a fixed resolution, so fitting a
//! Clip's peaks after a resize never rescans the source's samples.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Weak};

use vibez_core::audio_buffer::DecodedAudio;

/// Source frames summarised by one bucket.
const BUCKET_FRAMES: usize = 256;

/// Sources kept before the cache drops those no longer loaded.
const CACHE_SOURCES: usize = 64;

/// Extrema of every `BUCKET_FRAMES` frames of one source, across all of
/// its channels.
pub(super) struct SourcePeaks {
    buckets: Vec<(f32, f32)>,
}

#[cfg(test)]
thread_local! { pub(super) static SOURCE_SCANS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

struct CachedSource {
    /// Held weakly so the cache never keeps a source loaded, and so its
    /// address is never reused by another source while the entry exists.
    source: Weak<DecodedAudio>,
    peaks: Arc<SourcePeaks>,
}

thread_local! {
    static SOURCES: RefCell<HashMap<usize, CachedSource>> = RefCell::new(HashMap::new());
}

/// The summary of `audio`, scanned once per loaded source.
pub(super) fn source_peaks(audio: &Arc<DecodedAudio>) -> Arc<SourcePeaks> {
    let key = Arc::as_ptr(audio) as usize;
    if let Some(peaks) = SOURCES.with(|sources| {
        sources.borrow().get(&key).and_then(|cached| {
            cached
                .source
                .upgrade()
                .filter(|source| Arc::ptr_eq(source, audio))
                .map(|_| Arc::clone(&cached.peaks))
        })
    }) {
        return peaks;
    }

    let peaks = Arc::new(SourcePeaks::scan(audio));
    SOURCES.with(|sources| {
        let mut sources = sources.borrow_mut();
        if sources.len() >= CACHE_SOURCES {
            sources.retain(|_, cached| cached.source.strong_count() > 0);
        }
        sources.insert(
            key,
            CachedSource {
                source: Arc::downgrade(audio),
                peaks: Arc::clone(&peaks),
            },
        );
    });
    peaks
}

impl SourcePeaks {
    fn scan(audio: &DecodedAudio) -> Self {
        #[cfg(test)]
        SOURCE_SCANS.with(|scans| scans.set(scans.get() + 1));
        let frames = audio.num_frames();
        let buckets = (0..frames.div_ceil(BUCKET_FRAMES))
            .map(|bucket| {
                let start = bucket * BUCKET_FRAMES;
                scan_range(audio, start, (start + BUCKET_FRAMES).min(frames))
            })
            .collect();
        Self { buckets }
    }

    /// Extrema of source frames `start..end` of `audio`, the source this
    /// summary was scanned from, always including silence.
    ///
    /// Whole buckets come from the summary and only the partial buckets at
    /// either end are read from the samples, so the result is exact.
    pub(super) fn peak(&self, audio: &DecodedAudio, start: usize, end: usize) -> (f32, f32) {
        let end = end.min(audio.num_frames());
        if start >= end {
            return (0.0, 0.0);
        }
        let first_whole = start.div_ceil(BUCKET_FRAMES);
        let last_whole = end / BUCKET_FRAMES;
        if first_whole >= last_whole {
            return scan_range(audio, start, end);
        }
        let head = scan_range(audio, start, first_whole * BUCKET_FRAMES);
        let tail = scan_range(audio, last_whole * BUCKET_FRAMES, end);
        self.buckets[first_whole..last_whole]
            .iter()
            .chain([&head, &tail])
            .fold((0.0, 0.0), |(min, max), &(low, high)| {
                (min.min(low), max.max(high))
            })
    }
}

fn scan_range(audio: &DecodedAudio, start: usize, end: usize) -> (f32, f32) {
    (0..audio.num_channels()).fold((0.0f32, 0.0f32), |(min, max), channel| {
        let (low, high) = audio.peak_in_range(channel, start, end);
        (min.min(low), max.max(high))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(samples: Vec<f32>) -> Arc<DecodedAudio> {
        Arc::new(DecodedAudio {
            channels: vec![samples],
            sample_rate: 44_100,
        })
    }

    #[test]
    fn a_range_reads_only_its_own_frames_inside_a_summarised_bucket() {
        let mut samples = vec![0.1; 4 * BUCKET_FRAMES];
        samples[BUCKET_FRAMES + 10] = 0.9;
        samples[3 * BUCKET_FRAMES - 10] = -0.8;
        let audio = source(samples);
        let peaks = source_peaks(&audio);

        let start = BUCKET_FRAMES + 11;
        let end = 3 * BUCKET_FRAMES - 10;

        assert_eq!(peaks.peak(&audio, start, end), (0.0, 0.1));
        assert_eq!(peaks.peak(&audio, start - 1, end + 1), (-0.8, 0.9));
        assert_eq!(peaks.peak(&audio, 0, 4 * BUCKET_FRAMES), (-0.8, 0.9));
    }

    #[test]
    fn one_source_is_scanned_once() {
        let audio = source(vec![0.5; 10 * BUCKET_FRAMES]);
        let scans = SOURCE_SCANS.with(|scans| scans.get());

        let first = source_peaks(&audio);
        let again = source_peaks(&audio);

        assert!(Arc::ptr_eq(&first, &again));
        assert_eq!(SOURCE_SCANS.with(|scans| scans.get()), scans + 1);
    }
}
