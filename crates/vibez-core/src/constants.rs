pub const DEFAULT_SAMPLE_RATE: u32 = 44_100;
pub const DEFAULT_BUFFER_SIZE: usize = 512;
pub const DEFAULT_CHANNELS: usize = 2;
pub const DEFAULT_BPM: f64 = 120.0;
pub const MIN_BPM: f64 = 20.0;
pub const MAX_BPM: f64 = 999.0;

/// Maximum number of tracks.
pub const MAX_TRACKS: usize = 64;

/// Default track gain (unity).
pub const DEFAULT_TRACK_GAIN: f32 = 1.0;

/// Default track pan (center). Range 0.0 (left) to 1.0 (right).
pub const DEFAULT_TRACK_PAN: f32 = 0.5;

/// Ring buffer capacity for engine commands/events.
pub const RING_BUFFER_CAPACITY: usize = 1024;

/// UI tick rate in milliseconds (60fps).
pub const UI_TICK_MS: u64 = 16;

/// Maximum number of effects per track.
pub const MAX_EFFECTS_PER_TRACK: usize = 8;
