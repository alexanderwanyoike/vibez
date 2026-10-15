//! Carries fixed-size observations of native Delay timing without owning audio history.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DelayLineState {
    pub old_samples: f64,
    pub destination_samples: f64,
    pub progress: f64,
    pub target_samples: f64,
    pub pending_samples: Option<f64>,
    pub remaining_frames: u64,
    pub moving: bool,
    pub gliding: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DelayFilterState {
    pub enabled: f32,
    pub cutoff_hz: f64,
    pub moving: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DelayState {
    pub sample_rate: u32,
    pub bpm: f64,
    pub processed_frames: u64,
    pub observation_order: u64,
    pub targets: [f32; 17],
    pub lines: [DelayLineState; 2],
    pub mix: f32,
    pub feedback: f32,
    pub routing: f32,
    pub filters: [DelayFilterState; 2],
    pub duck_attenuation_db: f32,
    pub reference_moving: bool,
}
