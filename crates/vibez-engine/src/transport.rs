use vibez_core::constants::{DEFAULT_BPM, MAX_BPM, MIN_BPM};

/// Playback transport state.
///
/// Tracks whether the engine is playing or stopped, the current playback
/// position (in samples), tempo, and optionally the total audio length so
/// that `advance()` can clamp to the end of the loaded audio.
#[derive(Debug, Clone)]
pub struct Transport {
    /// Whether playback is active.
    playing: bool,
    /// Current position in samples (absolute frame offset).
    position: u64,
    /// Tempo in beats per minute.
    bpm: f64,
    /// Total length of the loaded audio in samples.  `None` means no audio is
    /// loaded, so `advance()` increments without clamping.
    audio_length: Option<u64>,
    /// Whether arrangement-level looping is active.
    loop_enabled: bool,
    /// Loop region start in samples.
    loop_start: u64,
    /// Loop region end in samples.
    loop_end: u64,
}

impl Transport {
    /// Create a new transport in the stopped state at position 0.
    pub fn new() -> Self {
        Self {
            playing: false,
            position: 0,
            bpm: DEFAULT_BPM,
            audio_length: None,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        }
    }

    /// Start playback.
    pub fn play(&mut self) {
        self.playing = true;
    }

    /// Stop playback.  The position is preserved.
    pub fn stop(&mut self) {
        self.playing = false;
    }

    /// Whether the transport is currently playing.
    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Current playback position in samples.
    pub fn position(&self) -> u64 {
        self.position
    }

    /// Current tempo in BPM.
    pub fn bpm(&self) -> f64 {
        self.bpm
    }

    /// Seek to an absolute sample position.  If `audio_length` is set the
    /// position is clamped to `[0, audio_length]`.
    pub fn seek(&mut self, pos: u64) {
        self.position = match self.audio_length {
            Some(len) => pos.min(len),
            None => pos,
        };
    }

    /// Set the tempo, clamped to `[MIN_BPM, MAX_BPM]`.
    pub fn set_bpm(&mut self, bpm: f64) {
        self.bpm = bpm.clamp(MIN_BPM, MAX_BPM);
    }

    /// Set the total audio length.  Passing `None` removes the length
    /// constraint.  If the current position exceeds the new length it is
    /// clamped.
    pub fn set_audio_length(&mut self, length: Option<u64>) {
        self.audio_length = length;
        if let Some(len) = length {
            self.position = self.position.min(len);
        }
    }

    /// The total audio length, if set.
    pub fn audio_length(&self) -> Option<u64> {
        self.audio_length
    }

    /// Whether arrangement-level looping is enabled.
    pub fn loop_enabled(&self) -> bool {
        self.loop_enabled
    }

    /// Loop region start in samples.
    pub fn loop_start(&self) -> u64 {
        self.loop_start
    }

    /// Loop region end in samples.
    pub fn loop_end(&self) -> u64 {
        self.loop_end
    }

    /// The active arrangement loop region as a `(start, end)` pair if
    /// looping is enabled and the region is non-empty. Used by the
    /// mixer to wrap clip reads mid-block instead of spilling audio
    /// past the loop boundary.
    pub fn active_loop_region(&self) -> Option<(u64, u64)> {
        if self.loop_enabled && self.loop_end > self.loop_start {
            Some((self.loop_start, self.loop_end))
        } else {
            None
        }
    }

    /// Enable or disable arrangement-level looping.
    pub fn set_loop_enabled(&mut self, enabled: bool) {
        self.loop_enabled = enabled;
    }

    /// Set the loop region (start and end in samples).
    pub fn set_loop_region(&mut self, start: u64, end: u64) {
        self.loop_start = start;
        self.loop_end = end;
    }

    /// Advance the transport by `frames` samples and return the new position.
    ///
    /// If the transport is stopped, the position is unchanged.  If
    /// `audio_length` is set, the position is clamped so it never exceeds the
    /// audio length.
    pub fn advance(&mut self, frames: u64) -> u64 {
        if !self.playing {
            return self.position;
        }

        self.position = self.position.saturating_add(frames);

        // Arrangement loop takes priority over audio_length auto-stop
        if self.loop_enabled && self.loop_end > self.loop_start && self.position >= self.loop_end {
            let overshoot = self.position - self.loop_end;
            let loop_len = self.loop_end - self.loop_start;
            self.position = self.loop_start + (overshoot % loop_len);
            return self.position;
        }

        if let Some(len) = self.audio_length {
            if self.position >= len {
                self.position = len;
                self.playing = false; // auto-stop at end
            }
        }

        self.position
    }
}

impl Default for Transport {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_transport_is_stopped_at_zero() {
        let t = Transport::new();
        assert!(!t.is_playing());
        assert_eq!(t.position(), 0);
        assert!((t.bpm() - DEFAULT_BPM).abs() < f64::EPSILON);
        assert_eq!(t.audio_length(), None);
    }

    #[test]
    fn play_and_stop() {
        let mut t = Transport::new();
        t.play();
        assert!(t.is_playing());
        t.stop();
        assert!(!t.is_playing());
    }

    #[test]
    fn seek_without_audio_length() {
        let mut t = Transport::new();
        t.seek(10_000);
        assert_eq!(t.position(), 10_000);
        t.seek(u64::MAX);
        assert_eq!(t.position(), u64::MAX);
    }

    #[test]
    fn seek_with_audio_length_clamps() {
        let mut t = Transport::new();
        t.set_audio_length(Some(44_100));
        t.seek(50_000);
        assert_eq!(t.position(), 44_100);
        t.seek(22_050);
        assert_eq!(t.position(), 22_050);
    }

    #[test]
    fn advance_when_stopped_does_nothing() {
        let mut t = Transport::new();
        t.seek(100);
        let pos = t.advance(512);
        assert_eq!(pos, 100);
        assert_eq!(t.position(), 100);
    }

    #[test]
    fn advance_when_playing() {
        let mut t = Transport::new();
        t.play();
        let pos = t.advance(512);
        assert_eq!(pos, 512);
        let pos = t.advance(256);
        assert_eq!(pos, 768);
    }

    #[test]
    fn advance_clamps_to_audio_length() {
        let mut t = Transport::new();
        t.set_audio_length(Some(1000));
        t.play();
        let pos = t.advance(800);
        assert_eq!(pos, 800);
        assert!(t.is_playing());

        // This would go to 1800, but should clamp to 1000 and auto-stop.
        let pos = t.advance(1000);
        assert_eq!(pos, 1000);
        assert!(!t.is_playing()); // auto-stopped
    }

    #[test]
    fn advance_auto_stops_at_exact_end() {
        let mut t = Transport::new();
        t.set_audio_length(Some(512));
        t.play();
        let pos = t.advance(512);
        assert_eq!(pos, 512);
        assert!(!t.is_playing());
    }

    #[test]
    fn set_bpm_clamps() {
        let mut t = Transport::new();
        t.set_bpm(10.0); // below MIN_BPM
        assert!((t.bpm() - MIN_BPM).abs() < f64::EPSILON);

        t.set_bpm(2000.0); // above MAX_BPM
        assert!((t.bpm() - MAX_BPM).abs() < f64::EPSILON);

        t.set_bpm(140.0);
        assert!((t.bpm() - 140.0).abs() < f64::EPSILON);
    }

    #[test]
    fn set_audio_length_clamps_position() {
        let mut t = Transport::new();
        t.seek(5000);
        t.set_audio_length(Some(2000));
        assert_eq!(t.position(), 2000);
    }

    #[test]
    fn remove_audio_length() {
        let mut t = Transport::new();
        t.set_audio_length(Some(1000));
        t.set_audio_length(None);
        assert_eq!(t.audio_length(), None);
        t.seek(999_999);
        assert_eq!(t.position(), 999_999);
    }

    #[test]
    fn advance_saturates_without_audio_length() {
        let mut t = Transport::new();
        t.seek(u64::MAX - 10);
        t.play();
        let pos = t.advance(100);
        assert_eq!(pos, u64::MAX);
    }

    #[test]
    fn default_is_same_as_new() {
        let a = Transport::new();
        let b = Transport::default();
        assert_eq!(a.position(), b.position());
        assert_eq!(a.is_playing(), b.is_playing());
        assert!((a.bpm() - b.bpm()).abs() < f64::EPSILON);
    }

    #[test]
    fn loop_wraps_at_end() {
        let mut t = Transport::new();
        t.set_loop_enabled(true);
        t.set_loop_region(1000, 2000);
        t.seek(1900);
        t.play();
        let pos = t.advance(200); // 1900 + 200 = 2100 → wraps to 1000 + 100 = 1100
        assert_eq!(pos, 1100);
        assert!(t.is_playing());
    }

    #[test]
    fn loop_no_wrap_when_disabled() {
        let mut t = Transport::new();
        t.set_loop_enabled(false);
        t.set_loop_region(1000, 2000);
        t.seek(1900);
        t.play();
        let pos = t.advance(200);
        assert_eq!(pos, 2100); // no wrap
    }

    #[test]
    fn loop_overshoot_modulo() {
        let mut t = Transport::new();
        t.set_loop_enabled(true);
        t.set_loop_region(0, 100);
        t.seek(50);
        t.play();
        // 50 + 300 = 350, overshoot = 250, 250 % 100 = 50 → position = 50
        let pos = t.advance(300);
        assert_eq!(pos, 50);
    }

    #[test]
    fn loop_priority_over_audio_length() {
        let mut t = Transport::new();
        t.set_audio_length(Some(2000));
        t.set_loop_enabled(true);
        t.set_loop_region(500, 1500);
        t.seek(1400);
        t.play();
        // Would auto-stop at 2000 without loop, but loop wraps first
        let pos = t.advance(200); // 1400 + 200 = 1600 → wraps to 500 + 100 = 600
        assert_eq!(pos, 600);
        assert!(t.is_playing()); // did NOT auto-stop
    }

    #[test]
    fn loop_accessors() {
        let mut t = Transport::new();
        assert!(!t.loop_enabled());
        assert_eq!(t.loop_start(), 0);
        assert_eq!(t.loop_end(), 0);

        t.set_loop_enabled(true);
        t.set_loop_region(100, 500);
        assert!(t.loop_enabled());
        assert_eq!(t.loop_start(), 100);
        assert_eq!(t.loop_end(), 500);
    }
}
