//! Compact timing inputs shared by the native and batch instrument renderers.

use vibez_core::perform::SwingAmount;
use vibez_core::time::TempoMap;

pub(crate) struct InstrumentRenderContext<'a> {
    pub pos: u64,
    pub repeat_pos: u64,
    pub frames: usize,
    pub channels: usize,
    pub tempo_map: &'a TempoMap,
    pub project_swing: SwingAmount,
}

#[derive(Clone, Copy)]
pub(super) struct InstrumentRenderBlock {
    pub pos: u64,
    pub repeat_pos: u64,
    pub frames: usize,
    pub channels: usize,
    pub samples_per_beat: f64,
    pub bpm: f64,
    pub sample_rate: u32,
    pub swing: SwingAmount,
}
