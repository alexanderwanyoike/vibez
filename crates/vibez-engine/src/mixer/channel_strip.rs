//! EngineTrack channel-strip state, source rendering, and effects.

use super::*;
use crate::playback_source::ArrangementPlaybackSource;
use vibez_core::constants::{DEFAULT_TRACK_GAIN, DEFAULT_TRACK_PAN};

impl EngineTrack {
    pub fn new(id: TrackId) -> Self {
        Self::with_playback_source(id, ArrangementPlaybackSource::prepare_empty())
    }

    /// Wrap already-resident timeline content in a project-owned channel strip.
    pub fn with_playback_source(id: TrackId, playback_source: PreparedPlaybackSource) -> Self {
        Self {
            id,
            playback_source: Box::new(playback_source),
            section_playback_source: Box::new(PreparedPlaybackSource::default()),
            gain: DEFAULT_TRACK_GAIN,
            pan: DEFAULT_TRACK_PAN,
            mute: false,
            solo: false,
            swing_offset: None,
            automation_swing_offset: None,
            mix_buffer: Vec::new(),
            effects: Vec::new(),
            sends: Vec::new(),
            instrument: None,
            timed_note_ons: Vec::new(),
            timed_note_offs: Vec::new(),
            note_repeats: TrackNoteRepeats::default(),
            active_notes: 0,
            suppress_source_notes: false,
        }
    }

    /// Apply automation at `beat`; return gain and pan mix overrides.
    pub fn apply_automation(&mut self, beat: f64) -> (Option<f32>, Option<f32>) {
        use vibez_core::automation::AutomationTarget;
        let mut gain = None;
        let mut pan = None;
        let mut swing_offset = None;
        for lane_idx in 0..self.playback_source.automation.len() {
            let lane = &self.playback_source.automation[lane_idx];
            let Some(value) = lane.value_at(beat) else {
                continue;
            };
            match lane.target {
                // Gain's native range is 0..2; pan is already 0..1.
                AutomationTarget::TrackGain => gain = Some(value * 2.0),
                AutomationTarget::TrackPan => pan = Some(value),
                AutomationTarget::TrackSwingOffset => {
                    swing_offset = Some(SwingOffset::from_normalized(value));
                }
                AutomationTarget::EffectParam {
                    effect_id,
                    param_index,
                } => {
                    if let Some(slot) = self.effects.iter_mut().find(|e| e.id == effect_id) {
                        // Lanes are normalized 0..1; parameters live in
                        // their native descriptor range.
                        let native = match slot.effect.param_descriptors().get(param_index) {
                            Some(d) => d.min + value * (d.max - d.min),
                            None => value,
                        };
                        slot.effect.set_param(param_index, native);
                    }
                }
                AutomationTarget::InstrumentParam { param_index } => {
                    if let Some(instrument) = self.instrument.as_mut() {
                        let native = match instrument.param_descriptors().get(param_index) {
                            Some(d) => d.min + value * (d.max - d.min),
                            None => value,
                        };
                        instrument.set_param(param_index, native);
                    }
                }
                AutomationTarget::PluginParam { .. } => {}
                AutomationTarget::Send { bus_id } => {
                    // Send range is native 0..1, so write the value in place.
                    match self.sends.iter_mut().find(|(b, _)| *b == bus_id) {
                        Some(send) => send.1 = value,
                        None => self.sends.push((bus_id, value)),
                    }
                }
            }
        }
        self.automation_swing_offset = swing_offset;
        (gain, pan)
    }

    /// Zero the mix buffer: an idle block for a track with no source
    /// signal, so the effect chain can still run (tails, queued
    /// plugin param changes).
    pub fn clear_buffer(&mut self, frames: usize, channels: usize) {
        let buf_size = frames * channels;
        self.ensure_buffer(buf_size);
        for s in self.mix_buffer[..buf_size].iter_mut() {
            *s = 0.0;
        }
    }

    /// Send note-offs for every sounding note. Call whenever the
    /// playhead moves discontinuously; the offs reach the instrument
    /// immediately (built-ins) or on its next render (plugins).
    pub fn flush_notes(&mut self) {
        self.reset_groove_latches();
        if self.active_notes == 0 {
            return;
        }
        if let Some(instrument) = self.instrument.as_mut() {
            for pitch in 0..128u8 {
                if self.active_notes & (1u128 << pitch) != 0 {
                    instrument.note_off(pitch);
                }
            }
        }
        self.active_notes = 0;
    }

    /// Ensure the mix buffer has at least `size` elements.
    pub fn ensure_buffer(&mut self, size: usize) {
        if self.mix_buffer.len() < size {
            self.mix_buffer.resize(size, 0.0);
        }
    }

    /// Render all active clips into the mix_buffer for the given position.
    /// Returns `true` if any audio was rendered.
    ///
    /// `loop_region` is the active arrangement loop (if any). When
    /// provided, any frame within this block whose global position
    /// would land past `loop_end` is wrapped back to `loop_start +
    /// overshoot` before clip content is looked up. Without this, a
    /// block that straddles the loop boundary would play clip audio
    /// past the loop end before the transport wraps on the next
    /// block — which surfaces as a "double beat" when the clip
    /// extends slightly past the looped region (common with warped
    /// samples that aren't exactly one bar).
    pub fn render(
        &mut self,
        pos: u64,
        frames: usize,
        channels: usize,
        loop_region: Option<(u64, u64)>,
    ) -> bool {
        let buf_size = frames * channels;
        self.ensure_buffer(buf_size);
        self.playback_source.render_audio(
            &mut self.mix_buffer[..buf_size],
            pos,
            frames,
            channels,
            loop_region,
        )
    }

    pub fn process_effects(&mut self, frames: usize, channels: usize) {
        let buf_size = frames * channels;
        if buf_size == 0 {
            return;
        }
        for slot in &mut self.effects {
            if !slot.bypass {
                slot.effect
                    .process(&mut self.mix_buffer[..buf_size], channels);
            }
        }
    }
}
