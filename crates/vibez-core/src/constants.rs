pub const DEFAULT_SAMPLE_RATE: u32 = 44_100;
pub const DEFAULT_BUFFER_SIZE: usize = 512;
pub const DEFAULT_CHANNELS: usize = 2;
pub const DEFAULT_BPM: f64 = 120.0;
pub const MIN_BPM: f64 = 20.0;
pub const MAX_BPM: f64 = 999.0;

/// Ring buffer capacity for engine commands/events.
pub const RING_BUFFER_CAPACITY: usize = 1024;

/// UI tick rate in milliseconds (60fps).
pub const UI_TICK_MS: u64 = 16;
