//! Clip-local playback geometry shared by audio, MIDI, engine, and UI.

/// Playback geometry measured in source frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameClipTimeline {
    pub start: u64,
    pub loop_start: u64,
    pub loop_end: u64,
    pub duration: u64,
    pub loop_enabled: bool,
}

impl FrameClipTimeline {
    pub const MIN_START_GAP: u64 = 1;

    pub const fn new(
        start: u64,
        loop_start: u64,
        loop_end: u64,
        duration: u64,
        loop_enabled: bool,
    ) -> Self {
        Self {
            start,
            loop_start,
            loop_end,
            duration,
            loop_enabled,
        }
    }

    pub fn is_looping(self) -> bool {
        self.has_loop_region() && self.start < self.loop_end
    }

    fn has_loop_region(self) -> bool {
        self.loop_enabled && self.loop_end > self.loop_start
    }

    pub fn source_at(self, play_position: u64) -> u64 {
        let raw = self.start.saturating_add(play_position);
        if self.is_looping() && raw >= self.loop_end {
            self.loop_start + (raw - self.loop_end) % (self.loop_end - self.loop_start)
        } else {
            raw
        }
    }

    pub fn first_wrap(self) -> Option<u64> {
        self.is_looping().then(|| self.loop_end - self.start)
    }

    pub fn wraps(self) -> FrameOccurrences {
        FrameOccurrences {
            initial: None,
            next_repeat: self
                .first_wrap()
                .filter(|position| *position < self.duration),
            repeat_length: self.loop_end.saturating_sub(self.loop_start),
            duration: self.duration,
        }
    }

    pub fn occurrences_of(self, source_position: u64) -> FrameOccurrences {
        let initial = (source_position >= self.start
            && (!self.is_looping() || source_position < self.loop_end))
            .then(|| source_position - self.start)
            .filter(|position| *position < self.duration);
        let repeat = (self.is_looping()
            && source_position >= self.loop_start
            && source_position < self.loop_end)
            .then(|| self.loop_end - self.start + source_position - self.loop_start)
            .filter(|position| *position < self.duration);
        FrameOccurrences {
            initial,
            next_repeat: repeat,
            repeat_length: self.loop_end.saturating_sub(self.loop_start),
            duration: self.duration,
        }
    }

    pub fn clamp_start(self, candidate: u64, source_start: u64, source_end: u64) -> u64 {
        self.clamp_start_with_gap(candidate, source_start, source_end, Self::MIN_START_GAP)
    }

    pub fn clamp_start_with_gap(
        self,
        candidate: u64,
        source_start: u64,
        source_end: u64,
        minimum_gap: u64,
    ) -> u64 {
        let marker_end = if self.loop_enabled {
            source_end.min(self.loop_end)
        } else {
            source_end
        };
        candidate.clamp(
            source_start,
            marker_end
                .saturating_sub(minimum_gap.max(Self::MIN_START_GAP))
                .max(source_start),
        )
    }

    pub fn accepts_start(self, candidate: u64, source_start: u64, source_end: u64) -> bool {
        candidate == self.clamp_start(candidate, source_start, source_end)
            && candidate < source_end
            && (!self.loop_enabled || candidate < self.loop_end)
    }
}

pub struct FrameOccurrences {
    initial: Option<u64>,
    next_repeat: Option<u64>,
    repeat_length: u64,
    duration: u64,
}

impl FrameOccurrences {
    pub fn starting_after(mut self, boundary: u64) -> Self {
        if self.initial.is_some_and(|position| position <= boundary) {
            self.initial = None;
        }
        if let Some(next) = self.next_repeat {
            if next <= boundary && self.repeat_length > 0 {
                let skipped = (boundary - next) / self.repeat_length + 1;
                self.next_repeat = next.checked_add(skipped.saturating_mul(self.repeat_length));
            }
        }
        self
    }
}

impl Iterator for FrameOccurrences {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(initial) = self.initial.take() {
            return Some(initial);
        }
        let current = self
            .next_repeat
            .filter(|position| *position < self.duration)?;
        self.next_repeat = current.checked_add(self.repeat_length);
        Some(current)
    }
}

/// Playback geometry measured in musical beats.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BeatClipTimeline {
    pub start: f64,
    pub loop_start: f64,
    pub loop_end: f64,
    pub duration: f64,
    pub loop_enabled: bool,
}

impl BeatClipTimeline {
    pub const MIN_START_GAP: f64 = 0.01;

    pub const fn new(
        start: f64,
        loop_start: f64,
        loop_end: f64,
        duration: f64,
        loop_enabled: bool,
    ) -> Self {
        Self {
            start,
            loop_start,
            loop_end,
            duration,
            loop_enabled,
        }
    }

    pub fn is_looping(self) -> bool {
        self.has_loop_region() && self.start.is_finite() && self.start < self.loop_end
    }

    fn has_loop_region(self) -> bool {
        self.loop_enabled
            && self.loop_end.is_finite()
            && self.loop_start.is_finite()
            && self.loop_end > self.loop_start
    }

    pub fn source_at(self, play_position: f64) -> f64 {
        let raw = self.start + play_position;
        if self.is_looping() && raw >= self.loop_end {
            self.loop_start + (raw - self.loop_end) % (self.loop_end - self.loop_start)
        } else {
            raw
        }
    }

    pub fn first_wrap(self) -> Option<f64> {
        self.is_looping().then_some(self.loop_end - self.start)
    }

    pub fn wraps(self) -> BeatOccurrences {
        BeatOccurrences {
            initial: None,
            next_repeat: self
                .first_wrap()
                .filter(|position| *position < self.duration),
            repeat_length: self.loop_end - self.loop_start,
            duration: self.duration,
        }
    }

    pub fn occurrences_of(self, source_position: f64) -> BeatOccurrences {
        let valid_source = source_position.is_finite() && source_position >= 0.0;
        let initial = (valid_source
            && source_position >= self.start
            && (!self.is_looping() || source_position < self.loop_end))
            .then_some(source_position - self.start)
            .filter(|position| *position >= 0.0 && *position < self.duration);
        let repeat = (valid_source
            && self.is_looping()
            && source_position >= self.loop_start
            && source_position < self.loop_end)
            .then_some(self.loop_end - self.start + source_position - self.loop_start)
            .filter(|position| *position >= 0.0 && *position < self.duration);
        BeatOccurrences {
            initial,
            next_repeat: repeat,
            repeat_length: self.loop_end - self.loop_start,
            duration: self.duration,
        }
    }

    pub fn clamp_start(self, candidate: f64, source_start: f64, source_end: f64) -> f64 {
        self.clamp_start_with_gap(candidate, source_start, source_end, Self::MIN_START_GAP)
    }

    pub fn clamp_start_with_gap(
        self,
        candidate: f64,
        source_start: f64,
        source_end: f64,
        minimum_gap: f64,
    ) -> f64 {
        let marker_end = if self.loop_enabled && self.loop_end.is_finite() {
            source_end.min(self.loop_end)
        } else {
            source_end
        };
        candidate.clamp(
            source_start,
            (marker_end - minimum_gap.max(Self::MIN_START_GAP)).max(source_start),
        )
    }

    pub fn accepts_start(self, candidate: f64, source_start: f64, source_end: f64) -> bool {
        candidate.is_finite()
            && candidate == self.clamp_start(candidate, source_start, source_end)
            && candidate < source_end
            && (!self.loop_enabled || candidate < self.loop_end)
    }
}

pub struct BeatOccurrences {
    initial: Option<f64>,
    next_repeat: Option<f64>,
    repeat_length: f64,
    duration: f64,
}

impl BeatOccurrences {
    pub fn starting_after(mut self, boundary: f64) -> Self {
        if self.initial.is_some_and(|position| position <= boundary) {
            self.initial = None;
        }
        if let Some(next) = self.next_repeat {
            if next <= boundary && self.repeat_length > 0.0 {
                let skipped = ((boundary - next) / self.repeat_length).floor() + 1.0;
                self.next_repeat = Some(next + skipped * self.repeat_length);
            }
        }
        self
    }
}

impl Iterator for BeatOccurrences {
    type Item = f64;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(initial) = self.initial.take() {
            return Some(initial);
        }
        let current = self
            .next_repeat
            .filter(|position| *position < self.duration)?;
        self.next_repeat = Some(current + self.repeat_length);
        Some(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intro_plays_once_then_the_loop_repeats() {
        let frames = FrameClipTimeline::new(20, 10, 30, 50, true);
        assert_eq!(
            (0..7).map(|i| frames.source_at(i * 10)).collect::<Vec<_>>(),
            [20, 10, 20, 10, 20, 10, 20]
        );
        assert_eq!(frames.occurrences_of(10).collect::<Vec<_>>(), [10, 30]);

        let beats = BeatClipTimeline::new(2.0, 1.0, 3.0, 5.0, true);
        assert_eq!(beats.occurrences_of(1.0).collect::<Vec<_>>(), [1.0, 3.0]);
        assert_eq!(
            beats.occurrences_of(2.0).collect::<Vec<_>>(),
            [0.0, 2.0, 4.0]
        );
    }

    #[test]
    fn frame_and_beat_flavours_share_the_same_integer_geometry() {
        let frames = FrameClipTimeline::new(2, 1, 4, 12, true);
        let beats = BeatClipTimeline::new(2.0, 1.0, 4.0, 12.0, true);
        for play_position in 0..12 {
            assert_eq!(
                frames.source_at(play_position) as f64,
                beats.source_at(play_position as f64)
            );
        }
        for source_position in 0..4 {
            let frame_occurrences = frames
                .occurrences_of(source_position)
                .map(|position| position as f64)
                .collect::<Vec<_>>();
            assert_eq!(
                frame_occurrences,
                beats
                    .occurrences_of(source_position as f64)
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn ranged_occurrences_jump_without_scanning_previous_repeats() {
        let beats = BeatClipTimeline::new(0.0, 0.0, 1.0, 1000.0, true);
        assert_eq!(
            beats.occurrences_of(0.25).starting_after(998.0).next(),
            Some(998.25)
        );
    }

    #[test]
    fn clamp_start_uses_each_flavours_native_minimum_gap() {
        let frames = FrameClipTimeline::new(0, 0, 100, 100, true);
        assert_eq!(frames.clamp_start(100, 0, 100), 99);
        let beats = BeatClipTimeline::new(0.0, 0.0, 4.0, 4.0, true);
        assert_eq!(beats.clamp_start(4.0, 0.0, 4.0), 3.99);
    }
}
