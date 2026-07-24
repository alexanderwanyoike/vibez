use vibez_core::id::TrackId;
use vibez_core::perform::{NoteRepeatRate, SwingAmount, SwingOffset};
use vibez_core::time::TempoMap;
use vibez_instruments::Instrument;

use crate::note_repeat::{NoteRepeatTrigger, TrackNoteRepeats};
use crate::playback_source::PreparedPlaybackSource;
pub use crate::playback_source::{EngineClip, EngineNoteClip};
mod channel_strip;
mod effect_slot;
mod groove;
mod note_repeat;
mod pan;
mod render_context;
pub use effect_slot::EffectSlot;
use groove::{crossed_beat, mapped_note_end, mapped_note_start};
pub use pan::{any_solo, balance_pan, equal_power_pan};
use render_context::InstrumentRenderBlock;
pub(crate) use render_context::InstrumentRenderContext;

const MUTE_RAMP_FRAMES: u32 = 64;

#[derive(Debug, Clone, Copy)]
struct MuteRamp {
    gain: f32,
    target: f32,
    remaining: u32,
}

impl Default for MuteRamp {
    fn default() -> Self {
        Self {
            gain: 1.0,
            target: 1.0,
            remaining: 0,
        }
    }
}

impl MuteRamp {
    fn set_muted(&mut self, muted: bool, immediate: bool) {
        let target = if muted { 0.0 } else { 1.0 };
        if immediate {
            self.gain = target;
            self.target = target;
            self.remaining = 0;
        } else if self.target != target {
            self.target = target;
            self.remaining = MUTE_RAMP_FRAMES;
        }
    }

    fn next_gain(&mut self) -> f32 {
        if self.remaining == 0 {
            return self.gain;
        }
        self.gain += (self.target - self.gain) / self.remaining as f32;
        self.remaining -= 1;
        if self.remaining == 0 {
            self.gain = self.target;
        }
        self.gain
    }
}

/// Runtime manual-overrides for automated parameters. The fixed-capacity
/// storage keeps gesture changes allocation-free on the audio thread.
#[derive(Debug, Default)]
struct AutomationOverrides {
    targets: [Option<vibez_core::automation::AutomationTarget>; 16],
}

impl AutomationOverrides {
    fn contains(&self, target: vibez_core::automation::AutomationTarget) -> bool {
        self.targets.contains(&Some(target))
    }

    fn set(&mut self, target: vibez_core::automation::AutomationTarget, overridden: bool) -> bool {
        let existing = self.targets.iter().position(|entry| *entry == Some(target));
        match (existing, overridden) {
            (Some(_), true) | (None, false) => false,
            (Some(index), false) => {
                self.targets[index] = None;
                true
            }
            (None, true) => {
                let Some(slot) = self.targets.iter_mut().find(|entry| entry.is_none()) else {
                    return false;
                };
                *slot = Some(target);
                true
            }
        }
    }
}

/// A track as it exists at runtime in the engine.
pub struct EngineTrack {
    pub id: TrackId,
    /// Time-based content feeding this shared channel strip. It is prepared
    /// outside the callback before any future source switch.
    pub playback_source: Box<PreparedPlaybackSource>,
    /// Resident Perform source. The engine swaps this pointer with
    /// `playback_source` only around Section rendering, preserving Arrange as
    /// the editable source while sharing the exact same renderer.
    pub section_playback_source: Box<PreparedPlaybackSource>,
    pub gain: f32,
    pub pan: f32,
    pub mute: bool,
    automation_mute: Option<bool>,
    automation_overrides: AutomationOverrides,
    mute_ramp: MuteRamp,
    pub solo: bool,
    pub swing_offset: Option<SwingOffset>,
    /// Block-evaluated automation override; manual FOLLOW/offset remains intact.
    automation_swing_offset: Option<SwingOffset>,
    /// Pre-allocated per-track mix buffer (interleaved stereo).
    pub mix_buffer: Vec<f32>,
    pub effects: Vec<EffectSlot>,
    /// Post-fader send amounts into bus channels: `(bus id, 0..1)`.
    /// Only regular tracks send; buses and the master never do, so
    /// the routing graph stays acyclic by construction.
    pub sends: Vec<(TrackId, f32)>,
    pub instrument: Option<Box<dyn Instrument>>,
    /// Scratch storage for batch rendering: (frame_offset, pitch, velocity)
    timed_note_ons: Vec<(u32, u8, u8)>,
    /// Scratch storage for batch rendering: (frame_offset, pitch)
    timed_note_offs: Vec<(u32, u8)>,
    note_repeats: TrackNoteRepeats,
    /// Clip pitches sounding on the instrument. Lets the engine kill
    /// hanging notes when the playhead jumps discontinuously
    /// (arrangement-loop wrap, seek, stop): the schedulers only see
    /// adjacent sample positions, so a jump would otherwise strand
    /// every sounding note in the "held down" state forever.
    active_notes: u128,
    pub(crate) suppress_source_notes: bool,
}

impl EngineTrack {
    pub(crate) fn render_instrument(
        &mut self,
        context: InstrumentRenderContext<'_>,
        on_repeat: &mut dyn FnMut(NoteRepeatTrigger),
    ) -> bool {
        let InstrumentRenderContext {
            pos,
            repeat_pos,
            frames,
            channels,
            tempo_map,
            project_swing,
        } = context;
        if self.instrument.is_none() {
            return false;
        }

        let buf_size = frames * channels;
        self.ensure_buffer(buf_size);
        for s in self.mix_buffer[..buf_size].iter_mut() {
            *s = 0.0;
        }

        let spb = tempo_map.samples_per_beat();
        if spb <= 0.0 {
            return false;
        }

        let batch = self.instrument.as_ref().unwrap().supports_batch_render();
        let swing = self.effective_swing(project_swing);
        let block = InstrumentRenderBlock {
            pos,
            repeat_pos,
            frames,
            channels,
            samples_per_beat: spb,
            bpm: tempo_map.bpm,
            sample_rate: tempo_map.sample_rate,
            swing,
        };

        if batch {
            self.render_instrument_batch(block, on_repeat)
        } else {
            self.render_instrument_per_frame(block, on_repeat)
        }
    }

    /// Batch render path: pre-collect all timed MIDI events, send them with
    /// frame offsets, then call `render()` once for the entire buffer.
    /// Used for external plugins (CLAP/VST3) that process efficiently in
    /// larger blocks.
    fn render_instrument_batch(
        &mut self,
        block: InstrumentRenderBlock,
        on_repeat: &mut dyn FnMut(NoteRepeatTrigger),
    ) -> bool {
        let InstrumentRenderBlock {
            pos,
            repeat_pos,
            frames,
            channels,
            samples_per_beat: spb,
            bpm,
            sample_rate,
            swing,
        } = block;
        let buf_size = frames * channels;
        let beat_step = 1.0 / spb;
        let mut rendered = false;

        // Pre-scan: collect all events with frame offsets
        for frame in 0..frames {
            let sample_pos = pos + frame as u64;
            let (triggers, count) =
                self.note_repeats
                    .triggers_at(repeat_pos + frame as u64, bpm, sample_rate, swing);
            for trigger in triggers.into_iter().take(count).flatten() {
                let instrument = self.instrument.as_mut().unwrap();
                instrument.note_off_at(trigger.pitch, frame as u32);
                instrument.note_on_at(trigger.pitch, trigger.velocity, frame as u32);
                on_repeat(trigger);
                rendered = true;
            }
            let current_beat = sample_pos as f64 / spb;
            let prev_beat = if sample_pos > 0 {
                (sample_pos - 1) as f64 / spb
            } else {
                -1.0
            };

            let note_clips = if self.suppress_source_notes {
                &[][..]
            } else {
                &self.playback_source.note_clips
            };
            for clip in note_clips {
                let clip_start_beat = clip.position_beats;
                let clip_end_beat = clip.position_beats + clip.duration_beats;

                let in_clip = current_beat >= clip_start_beat && current_beat < clip_end_beat;
                let was_in_clip = prev_beat >= clip_start_beat && prev_beat < clip_end_beat;

                if was_in_clip && !in_clip {
                    for note in &clip.notes {
                        self.timed_note_offs.push((frame as u32, note.pitch));
                    }
                    continue;
                }

                if !in_clip {
                    continue;
                }

                let local_beat = current_beat - clip_start_beat;
                let looping = clip.loop_enabled && clip.loop_end_beats > clip.loop_start_beats;
                let loop_len = if looping {
                    clip.loop_end_beats - clip.loop_start_beats
                } else {
                    0.0
                };

                let effective_local = if looping && local_beat >= clip.loop_end_beats {
                    clip.loop_start_beats + (local_beat - clip.loop_start_beats) % loop_len
                } else {
                    local_beat
                };

                let prev_effective_local = if !was_in_clip {
                    -1.0
                } else {
                    let prev_local = prev_beat - clip_start_beat;
                    if looping && prev_local >= clip.loop_end_beats {
                        clip.loop_start_beats + (prev_local - clip.loop_start_beats) % loop_len
                    } else {
                        prev_local
                    }
                };

                let wrapped = looping
                    && was_in_clip
                    && prev_effective_local > effective_local + beat_step * 0.5;
                let clip_swing =
                    clip.swing_for_pair(effective_local, swing, wrapped || !was_in_clip);

                if wrapped {
                    for note in &clip.notes {
                        self.timed_note_offs.push((frame as u32, note.pitch));
                    }
                    for note in &clip.notes {
                        let mapped_start = mapped_note_start(clip, note, clip_swing);
                        if mapped_start >= clip.loop_start_beats && mapped_start <= effective_local
                        {
                            self.timed_note_ons
                                .push((frame as u32, note.pitch, note.velocity));
                        }
                    }
                } else {
                    for (note_index, note) in clip.notes.iter().enumerate() {
                        let mapped_start = mapped_note_start(clip, note, clip_swing);
                        if mapped_start < clip.duration_beats
                            && crossed_beat(prev_effective_local, effective_local, mapped_start)
                        {
                            self.timed_note_ons
                                .push((frame as u32, note.pitch, note.velocity));
                        }
                        if crossed_beat(
                            prev_effective_local,
                            effective_local,
                            mapped_note_end(clip, note_index, clip_swing),
                        ) {
                            self.timed_note_offs.push((frame as u32, note.pitch));
                        }
                    }
                }
            }
        }

        // Send all events with timing, then render the whole buffer at once
        let instrument = self.instrument.as_mut().unwrap();

        for &(frame, pitch) in &self.timed_note_offs {
            instrument.note_off_at(pitch, frame);
        }
        for &(frame, pitch, vel) in &self.timed_note_ons {
            instrument.note_on_at(pitch, vel, frame);
            rendered = true;
        }
        // Update the sounding-note mask in event order: within one
        // block an off followed by an on at a later frame must leave
        // the note marked active.
        for &(_, pitch) in &self.timed_note_offs {
            self.active_notes &= !(1u128 << pitch);
        }
        for &(frame, pitch, _) in &self.timed_note_ons {
            let killed_later = self
                .timed_note_offs
                .iter()
                .any(|&(off_frame, off_pitch)| off_pitch == pitch && off_frame > frame);
            if !killed_later {
                self.active_notes |= 1u128 << pitch;
            }
        }

        instrument.render(&mut self.mix_buffer[..buf_size], channels);

        self.timed_note_ons.clear();
        self.timed_note_offs.clear();

        if !rendered {
            rendered = self.mix_buffer[..buf_size].iter().any(|&s| s != 0.0);
        }

        rendered
    }

    /// Per-frame render path: process one sample at a time with immediate
    /// note events. Used for built-in instruments (SubtractiveSynth, Sampler)
    /// which handle per-frame rendering efficiently.
    fn render_instrument_per_frame(
        &mut self,
        block: InstrumentRenderBlock,
        on_repeat: &mut dyn FnMut(NoteRepeatTrigger),
    ) -> bool {
        let InstrumentRenderBlock {
            pos,
            repeat_pos,
            frames,
            channels,
            samples_per_beat: spb,
            bpm,
            sample_rate,
            swing,
        } = block;
        let buf_size = frames * channels;
        let mut rendered = false;
        let mut note_ons: Vec<(u8, u8)> = Vec::new();
        let mut note_offs: Vec<u8> = Vec::new();
        let beat_step = 1.0 / spb;

        for frame in 0..frames {
            let sample_pos = pos + frame as u64;
            let (repeat_triggers, repeat_count) =
                self.note_repeats
                    .triggers_at(repeat_pos + frame as u64, bpm, sample_rate, swing);
            let current_beat = sample_pos as f64 / spb;
            let prev_beat = if sample_pos > 0 {
                (sample_pos - 1) as f64 / spb
            } else {
                -1.0
            };

            note_ons.clear();
            note_offs.clear();

            let note_clips = if self.suppress_source_notes {
                &[][..]
            } else {
                &self.playback_source.note_clips
            };
            for clip in note_clips {
                let clip_start_beat = clip.position_beats;
                let clip_end_beat = clip.position_beats + clip.duration_beats;

                let in_clip = current_beat >= clip_start_beat && current_beat < clip_end_beat;
                let was_in_clip = prev_beat >= clip_start_beat && prev_beat < clip_end_beat;

                if was_in_clip && !in_clip {
                    for note in &clip.notes {
                        note_offs.push(note.pitch);
                    }
                    continue;
                }

                if !in_clip {
                    continue;
                }

                let local_beat = current_beat - clip_start_beat;
                let looping = clip.loop_enabled && clip.loop_end_beats > clip.loop_start_beats;
                let loop_len = if looping {
                    clip.loop_end_beats - clip.loop_start_beats
                } else {
                    0.0
                };

                let effective_local = if looping && local_beat >= clip.loop_end_beats {
                    clip.loop_start_beats + (local_beat - clip.loop_start_beats) % loop_len
                } else {
                    local_beat
                };

                let prev_effective_local = if !was_in_clip {
                    -1.0
                } else {
                    let prev_local = prev_beat - clip_start_beat;
                    if looping && prev_local >= clip.loop_end_beats {
                        clip.loop_start_beats + (prev_local - clip.loop_start_beats) % loop_len
                    } else {
                        prev_local
                    }
                };

                let wrapped = looping
                    && was_in_clip
                    && prev_effective_local > effective_local + beat_step * 0.5;
                let clip_swing =
                    clip.swing_for_pair(effective_local, swing, wrapped || !was_in_clip);

                if wrapped {
                    for note in &clip.notes {
                        note_offs.push(note.pitch);
                    }
                    for note in &clip.notes {
                        let mapped_start = mapped_note_start(clip, note, clip_swing);
                        if mapped_start >= clip.loop_start_beats && mapped_start <= effective_local
                        {
                            note_ons.push((note.pitch, note.velocity));
                        }
                    }
                } else {
                    for (note_index, note) in clip.notes.iter().enumerate() {
                        let mapped_start = mapped_note_start(clip, note, clip_swing);
                        if mapped_start < clip.duration_beats
                            && crossed_beat(prev_effective_local, effective_local, mapped_start)
                        {
                            note_ons.push((note.pitch, note.velocity));
                        }
                        if crossed_beat(
                            prev_effective_local,
                            effective_local,
                            mapped_note_end(clip, note_index, clip_swing),
                        ) {
                            note_offs.push(note.pitch);
                        }
                    }
                }
            }

            let instrument = self.instrument.as_mut().unwrap();
            for pitch in &note_offs {
                instrument.note_off(*pitch);
                self.active_notes &= !(1u128 << *pitch);
            }
            for (pitch, vel) in &note_ons {
                instrument.note_on(*pitch, *vel);
                self.active_notes |= 1u128 << *pitch;
                rendered = true;
            }
            for trigger in repeat_triggers.into_iter().take(repeat_count).flatten() {
                instrument.note_off(trigger.pitch);
                instrument.note_on(trigger.pitch, trigger.velocity);
                on_repeat(trigger);
                rendered = true;
            }

            let start = frame * channels;
            let end = start + channels;
            instrument.render(&mut self.mix_buffer[start..end], channels);
        }

        if !rendered {
            rendered = self.mix_buffer[..buf_size].iter().any(|&s| s != 0.0);
        }

        rendered
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use vibez_core::audio_buffer::DecodedAudio;
    use vibez_core::id::ClipId;

    fn make_test_audio(frames: usize, value: f32) -> Arc<DecodedAudio> {
        Arc::new(DecodedAudio {
            channels: vec![vec![value; frames], vec![value; frames]],
            sample_rate: 44_100,
        })
    }

    #[test]
    fn pan_law_hard_left() {
        let (l, r) = equal_power_pan(0.0);
        assert!((l - 1.0).abs() < 1e-6);
        assert!(r.abs() < 1e-6);
    }

    #[test]
    fn pan_law_hard_right() {
        let (l, r) = equal_power_pan(1.0);
        assert!(l.abs() < 1e-6);
        assert!((r - 1.0).abs() < 1e-6);
    }

    #[test]
    fn pan_law_center() {
        let (l, r) = equal_power_pan(0.5);
        let expected = std::f32::consts::FRAC_1_SQRT_2; // ~0.707
        assert!((l - expected).abs() < 1e-6);
        assert!((r - expected).abs() < 1e-6);
    }

    #[test]
    fn balance_law_passes_center_at_unity() {
        assert_eq!(balance_pan(0.5), (1.0, 1.0));
        assert_eq!(balance_pan(0.0), (1.0, 0.0));
        assert_eq!(balance_pan(1.0), (0.0, 1.0));
        let (l, r) = balance_pan(0.25);
        assert_eq!(l, 1.0);
        assert!((r - 0.5).abs() < 1e-6);
    }

    #[test]
    fn pan_law_clamps_out_of_range() {
        let (l, r) = equal_power_pan(-0.5);
        assert!((l - 1.0).abs() < 1e-6);
        assert!(r.abs() < 1e-6);

        let (l, r) = equal_power_pan(1.5);
        assert!(l.abs() < 1e-6);
        assert!((r - 1.0).abs() < 1e-6);
    }

    #[test]
    fn single_clip_render() {
        let audio = make_test_audio(64, 0.5);
        let mut track = EngineTrack::new(TrackId::new());
        track.playback_source.clips.push(EngineClip {
            id: ClipId::new(),
            audio,
            position: 0,
            source_offset: 0,
            duration: 64,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        });

        let rendered = track.render(0, 8, 2, None);
        assert!(rendered);

        // Check that samples were written
        for i in 0..16 {
            assert!((track.mix_buffer[i] - 0.5).abs() < 1e-6);
        }
    }

    #[test]
    fn clip_with_offset() {
        let audio = Arc::new(DecodedAudio {
            channels: vec![
                (0..64).map(|i| i as f32 / 64.0).collect(),
                (0..64).map(|i| i as f32 / 64.0).collect(),
            ],
            sample_rate: 44_100,
        });

        let mut track = EngineTrack::new(TrackId::new());
        track.playback_source.clips.push(EngineClip {
            id: ClipId::new(),
            audio: audio.clone(),
            position: 0,
            source_offset: 10,
            duration: 20,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        });

        let rendered = track.render(0, 4, 2, None);
        assert!(rendered);

        // First frame should come from source_offset 10
        let expected = audio.sample(0, 10);
        assert!((track.mix_buffer[0] - expected).abs() < 1e-6);
    }

    #[test]
    fn no_clips_returns_false() {
        let mut track = EngineTrack::new(TrackId::new());
        let rendered = track.render(0, 8, 2, None);
        assert!(!rendered);
    }

    #[test]
    fn any_solo_detection() {
        let mut tracks = vec![
            EngineTrack::new(TrackId::new()),
            EngineTrack::new(TrackId::new()),
        ];
        assert!(!any_solo(&tracks));

        tracks[1].solo = true;
        assert!(any_solo(&tracks));
    }

    #[test]
    fn clip_not_active_before_position() {
        let audio = make_test_audio(100, 0.5);
        let clip = EngineClip {
            id: ClipId::new(),
            audio,
            position: 100,
            source_offset: 0,
            duration: 50,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        };
        // Requesting frames 0..50 — clip starts at 100, not active
        assert!(!clip.is_active(0, 50));
    }

    #[test]
    fn clip_active_when_overlapping() {
        let audio = make_test_audio(100, 0.5);
        let clip = EngineClip {
            id: ClipId::new(),
            audio,
            position: 40,
            source_offset: 0,
            duration: 50,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        };
        // Requesting frames 30..60 — clip is at 40..90, overlaps
        assert!(clip.is_active(30, 30));
    }

    #[test]
    fn clip_loop_renders_audio_past_source() {
        let audio = make_test_audio(100, 0.5);
        let mut track = EngineTrack::new(TrackId::new());
        track.playback_source.clips.push(EngineClip {
            id: ClipId::new(),
            audio,
            position: 0,
            source_offset: 0,
            duration: 200,
            loop_enabled: true,
            loop_start: 0,
            loop_end: 100,
        });

        // Render frames 100..108 (in looped region, past source length)
        let rendered = track.render(100, 8, 2, None);
        assert!(rendered);

        // Output should be source audio (0.5), not silence
        for i in 0..16 {
            assert!(
                (track.mix_buffer[i] - 0.5).abs() < 1e-6,
                "sample {i}: expected 0.5, got {}",
                track.mix_buffer[i]
            );
        }
    }

    #[test]
    fn clip_loop_wraps_correctly() {
        // Source: 100 frames with ascending values 0.0..0.99
        let audio = Arc::new(DecodedAudio {
            channels: vec![
                (0..100).map(|i| i as f32 / 100.0).collect(),
                (0..100).map(|i| i as f32 / 100.0).collect(),
            ],
            sample_rate: 44_100,
        });

        let mut track = EngineTrack::new(TrackId::new());
        track.playback_source.clips.push(EngineClip {
            id: ClipId::new(),
            audio: audio.clone(),
            position: 0,
            source_offset: 0,
            duration: 250,
            loop_enabled: true,
            loop_start: 0,
            loop_end: 100,
        });

        // Render all 250 frames
        let rendered = track.render(0, 250, 2, None);
        assert!(rendered);

        // frame 150 should wrap to source frame 50 (150 % 100 = 50)
        let val_150 = track.mix_buffer[150 * 2]; // left channel
        let expected_50 = audio.sample(0, 50);
        assert!(
            (val_150 - expected_50).abs() < 1e-6,
            "frame 150: expected {expected_50} (same as frame 50), got {val_150}",
        );

        // frame 200 should wrap to source frame 0 (200 % 100 = 0)
        let val_200 = track.mix_buffer[200 * 2];
        let expected_0 = audio.sample(0, 0);
        assert!(
            (val_200 - expected_0).abs() < 1e-6,
            "frame 200: expected {expected_0} (same as frame 0), got {val_200}",
        );
    }

    #[test]
    fn clip_loop_with_source_offset() {
        // Source: 100 frames ascending
        let audio = Arc::new(DecodedAudio {
            channels: vec![
                (0..100).map(|i| i as f32 / 100.0).collect(),
                (0..100).map(|i| i as f32 / 100.0).collect(),
            ],
            sample_rate: 44_100,
        });

        let mut track = EngineTrack::new(TrackId::new());
        track.playback_source.clips.push(EngineClip {
            id: ClipId::new(),
            audio: audio.clone(),
            position: 0,
            source_offset: 20,
            duration: 200,
            loop_enabled: true,
            loop_start: 20,
            loop_end: 100,
        });

        let rendered = track.render(0, 200, 2, None);
        assert!(rendered);

        // First frame maps to source frame 20
        let val_0 = track.mix_buffer[0];
        assert!(
            (val_0 - audio.sample(0, 20)).abs() < 1e-6,
            "frame 0: expected source[20]={}, got {}",
            audio.sample(0, 20),
            val_0
        );

        // frame 80: source_offset + 80 = 100 which is >= loop_end (100)
        // wraps: loop_start + (100 - loop_start) % loop_len = 20 + (100 - 20) % 80 = 20 + 0 = 20
        let val_80 = track.mix_buffer[80 * 2];
        assert!(
            (val_80 - audio.sample(0, 20)).abs() < 1e-6,
            "frame 80: expected source[20]={}, got {}",
            audio.sample(0, 20),
            val_80
        );
    }

    #[test]
    fn clip_no_loop_silence_past_source() {
        let audio = make_test_audio(100, 0.5);
        let mut track = EngineTrack::new(TrackId::new());
        track.playback_source.clips.push(EngineClip {
            id: ClipId::new(),
            audio,
            position: 0,
            source_offset: 0,
            duration: 200,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        });

        // Render frames 100..108 (past source, no loop)
        let rendered = track.render(100, 8, 2, None);
        assert!(rendered); // clip is still "active" (duration=200)

        // Output should be silence since DecodedAudio returns 0.0 for out-of-bounds
        for i in 0..16 {
            assert!(
                track.mix_buffer[i].abs() < 1e-6,
                "sample {i}: expected silence, got {}",
                track.mix_buffer[i]
            );
        }
    }

    #[test]
    fn arrangement_loop_wraps_mid_block_within_clip() {
        // Simulate the reported "double beat" bug: a clip that extends
        // past the arrangement loop end. Before the fix, the mixer
        // rendered the clip's audio past loop_end in the block that
        // straddled the boundary. With the loop-region wrap, frames
        // past loop_end are re-mapped to the start of the loop so
        // playback stays inside the bar.
        let audio = Arc::new(DecodedAudio {
            // Ramp from 0.0..1.0 so we can tell positions apart.
            channels: vec![
                (0..200).map(|i| i as f32 / 200.0).collect(),
                (0..200).map(|i| i as f32 / 200.0).collect(),
            ],
            sample_rate: 44_100,
        });
        let mut track = EngineTrack::new(TrackId::new());
        track.playback_source.clips.push(EngineClip {
            id: ClipId::new(),
            audio: Arc::clone(&audio),
            position: 0,
            source_offset: 0,
            duration: 200,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        });

        // Loop is [0, 100), clip extends to 200.
        // Render a block that crosses loop_end (pos=90, 20 frames -> 90..110).
        let rendered = track.render(90, 20, 2, Some((0, 100)));
        assert!(rendered);

        // Frames 0..10 (global 90..100) should read source[90..100].
        for f in 0..10 {
            let expected = (90 + f) as f32 / 200.0;
            let got = track.mix_buffer[f * 2];
            assert!(
                (got - expected).abs() < 1e-6,
                "pre-wrap frame {f}: expected {expected}, got {got}",
            );
        }
        // Frames 10..20 (global 100..110) should have wrapped to source[0..10],
        // NOT source[100..110]. This is the regression the fix prevents.
        for f in 10..20 {
            let expected = (f - 10) as f32 / 200.0;
            let got = track.mix_buffer[f * 2];
            assert!(
                (got - expected).abs() < 1e-6,
                "post-wrap frame {f}: expected source[{}]={}, got {} \
                 (would be {} without the fix)",
                f - 10,
                expected,
                got,
                (90 + f) as f32 / 200.0,
            );
        }
    }

    #[test]
    fn arrangement_loop_without_crossing_is_unchanged() {
        // When the block doesn't cross the loop boundary, behaviour
        // should be identical to rendering without a loop.
        let audio = Arc::new(DecodedAudio {
            channels: vec![
                (0..100).map(|i| i as f32 / 100.0).collect(),
                (0..100).map(|i| i as f32 / 100.0).collect(),
            ],
            sample_rate: 44_100,
        });
        let mut track = EngineTrack::new(TrackId::new());
        track.playback_source.clips.push(EngineClip {
            id: ClipId::new(),
            audio,
            position: 0,
            source_offset: 0,
            duration: 100,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        });
        let rendered = track.render(10, 20, 2, Some((0, 100)));
        assert!(rendered);
        for f in 0..20 {
            let expected = (10 + f) as f32 / 100.0;
            let got = track.mix_buffer[f * 2];
            assert!(
                (got - expected).abs() < 1e-6,
                "frame {f}: expected {expected}, got {got}",
            );
        }
    }
}
