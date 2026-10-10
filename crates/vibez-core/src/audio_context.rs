#[derive(Debug, Clone, Copy)]
pub struct DeviceAudioContext {
    pub musical_sample: u64,
    pub continuous_sample: u64,
    pub sample_rate: u32,
    pub bpm: f64,
    pub playing: bool,
}

impl DeviceAudioContext {
    pub fn seconds(self) -> f64 {
        self.musical_sample as f64 / self.sample_rate as f64
    }
    pub fn beats(self) -> f64 {
        self.seconds() * self.bpm / 60.0
    }
}
