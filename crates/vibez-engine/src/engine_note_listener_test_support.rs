//! An instrument that records every note it hears, at the sample it heard it, for engine tests.

use std::sync::{Arc, Mutex};
use vibez_core::midi::InstrumentKind;

/// `(absolute frame, on, pitch)` as the receiving instrument heard them.
pub(super) type Heard = Arc<Mutex<Vec<(u64, bool, u8)>>>;

pub(super) struct Listener {
    heard: Heard,
    rendered: u64,
    batch: bool,
}

impl Listener {
    pub(super) fn new(heard: Heard, batch: bool) -> Self {
        Self {
            heard,
            rendered: 0,
            batch,
        }
    }
}

impl vibez_instruments::Instrument for Listener {
    fn instrument_kind(&self) -> InstrumentKind {
        InstrumentKind::SubtractiveSynth
    }
    fn param_descriptors(&self) -> &'static [vibez_core::effect::ParamDescriptor] {
        &[]
    }
    fn set_param(&mut self, _: usize, _: f32) -> bool {
        false
    }
    fn get_param(&self, _: usize) -> f32 {
        0.0
    }
    fn note_on(&mut self, pitch: u8, _: u8) {
        self.heard
            .lock()
            .unwrap()
            .push((self.rendered, true, pitch));
    }
    fn note_off(&mut self, pitch: u8) {
        self.heard
            .lock()
            .unwrap()
            .push((self.rendered, false, pitch));
    }
    fn note_on_at(&mut self, pitch: u8, _: u8, frame: u32) {
        let at = self.rendered + u64::from(frame);
        self.heard.lock().unwrap().push((at, true, pitch));
    }
    fn note_off_at(&mut self, pitch: u8, frame: u32) {
        let at = self.rendered + u64::from(frame);
        self.heard.lock().unwrap().push((at, false, pitch));
    }
    fn render(&mut self, buffer: &mut [f32], channels: usize) {
        self.rendered += (buffer.len() / channels) as u64;
    }
    fn reset(&mut self) {}
    fn supports_batch_render(&self) -> bool {
        self.batch
    }
}
