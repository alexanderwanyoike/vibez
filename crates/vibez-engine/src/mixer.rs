use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::constants::{DEFAULT_TRACK_GAIN, DEFAULT_TRACK_PAN};
use vibez_core::id::{ClipId, EffectId, TrackId};
use vibez_core::midi::MidiNote;
use vibez_core::time::TempoMap;
use vibez_dsp::effect::AudioEffect;
use vibez_instruments::synth::SubtractiveSynth;

/// A clip as it exists at runtime in the engine (on the audio thread).
pub struct EngineClip {
    pub id: ClipId,
    pub audio: Arc<DecodedAudio>,
    /// Position on the timeline in samples.
    pub position: u64,
    /// Offset into the source audio in samples.
    pub source_offset: u64,
    /// Duration in samples.
    pub duration: u64,
}

impl EngineClip {
    /// The end position of this clip on the timeline.
    pub fn end_position(&self) -> u64 {
        self.position.saturating_add(self.duration)
    }

    /// Whether this clip is active (has audio to contribute) at the given
    /// global position for the given number of frames.
    pub fn is_active(&self, pos: u64, frames: u64) -> bool {
        let end = pos.saturating_add(frames);
        // Clip overlaps the [pos, pos+frames) range
        self.position < end && self.end_position() > pos
    }
}

pub struct EffectSlot {
    pub id: EffectId,
    pub effect: Box<dyn AudioEffect>,
    pub bypass: bool,
}

pub struct EngineNoteClip {
    pub id: ClipId,
    pub position_beats: f64,
    pub duration_beats: f64,
    pub notes: Vec<MidiNote>,
}

/// A track as it exists at runtime in the engine.
pub struct EngineTrack {
    pub id: TrackId,
    pub clips: Vec<EngineClip>,
    pub gain: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    /// Pre-allocated per-track mix buffer (interleaved stereo).
    pub mix_buffer: Vec<f32>,
    pub effects: Vec<EffectSlot>,
    pub note_clips: Vec<EngineNoteClip>,
    pub synth: Option<Box<SubtractiveSynth>>,
}

impl EngineTrack {
    pub fn new(id: TrackId) -> Self {
        Self {
            id,
            clips: Vec::new(),
            gain: DEFAULT_TRACK_GAIN,
            pan: DEFAULT_TRACK_PAN,
            mute: false,
            solo: false,
            mix_buffer: Vec::new(),
            effects: Vec::new(),
            note_clips: Vec::new(),
            synth: None,
        }
    }

    /// Ensure the mix buffer has at least `size` elements.
    pub fn ensure_buffer(&mut self, size: usize) {
        if self.mix_buffer.len() < size {
            self.mix_buffer.resize(size, 0.0);
        }
    }

    /// Render all active clips into the mix_buffer for the given position.
    /// Returns `true` if any audio was rendered.
    pub fn render(&mut self, pos: u64, frames: usize, channels: usize) -> bool {
        let buf_size = frames * channels;
        self.ensure_buffer(buf_size);

        // Zero the buffer
        for s in self.mix_buffer[..buf_size].iter_mut() {
            *s = 0.0;
        }

        let mut rendered_any = false;

        for clip in &self.clips {
            if !clip.is_active(pos, frames as u64) {
                continue;
            }

            rendered_any = true;
            let audio_channels = clip.audio.num_channels();

            for frame in 0..frames {
                let global_frame = pos as usize + frame;

                // Skip frames before clip starts
                if (global_frame as u64) < clip.position {
                    continue;
                }
                // Skip frames after clip ends
                if (global_frame as u64) >= clip.end_position() {
                    continue;
                }

                // Calculate the sample index into the source audio
                let clip_frame = global_frame - clip.position as usize;
                let source_frame = clip.source_offset as usize + clip_frame;

                for ch in 0..channels {
                    let sample = if ch < audio_channels {
                        clip.audio.sample(ch, source_frame)
                    } else if audio_channels > 0 {
                        clip.audio.sample(audio_channels - 1, source_frame)
                    } else {
                        0.0
                    };
                    self.mix_buffer[frame * channels + ch] += sample;
                }
            }
        }

        rendered_any
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

    pub fn render_instrument(
        &mut self,
        pos: u64,
        frames: usize,
        channels: usize,
        tempo_map: &TempoMap,
    ) -> bool {
        if self.synth.is_none() {
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

        let mut rendered = false;
        let mut note_ons: Vec<(u8, u8)> = Vec::new();
        let mut note_offs: Vec<u8> = Vec::new();

        for frame in 0..frames {
            let sample_pos = pos + frame as u64;

            note_ons.clear();
            note_offs.clear();

            for clip in &self.note_clips {
                let clip_start_beat = clip.position_beats;
                let clip_end_beat = clip.position_beats + clip.duration_beats;
                let current_beat = sample_pos as f64 / spb;

                if current_beat < clip_start_beat || current_beat >= clip_end_beat {
                    continue;
                }

                for note in &clip.notes {
                    let note_start_sample = ((clip_start_beat + note.start_beat) * spb) as u64;
                    let note_end_sample = ((clip_start_beat + note.end_beat()) * spb) as u64;

                    if sample_pos == note_start_sample {
                        note_ons.push((note.pitch, note.velocity));
                    }
                    if sample_pos == note_end_sample {
                        note_offs.push(note.pitch);
                    }
                }
            }

            let synth = self.synth.as_mut().unwrap();
            for (pitch, vel) in &note_ons {
                synth.note_on(*pitch, *vel);
                rendered = true;
            }
            for pitch in &note_offs {
                synth.note_off(*pitch);
            }

            let start = frame * channels;
            let end = start + channels;
            synth.render(&mut self.mix_buffer[start..end], channels);
        }

        if !rendered {
            rendered = self.mix_buffer[..buf_size].iter().any(|&s| s != 0.0);
        }

        rendered
    }
}

/// Equal-power pan law.
/// `pan` ranges from 0.0 (hard left) to 1.0 (hard right).
/// Returns `(left_gain, right_gain)`.
/// At center (0.5): both channels get ~0.707 (-3dB).
pub fn equal_power_pan(pan: f32) -> (f32, f32) {
    let pan = pan.clamp(0.0, 1.0);
    let angle = pan * std::f32::consts::FRAC_PI_2;
    (angle.cos(), angle.sin())
}

/// Returns `true` if any track in the slice has solo enabled.
pub fn any_solo(tracks: &[EngineTrack]) -> bool {
    tracks.iter().any(|t| t.solo)
}

/// Calculate the total length of audio across all tracks (max clip end position).
pub fn calculate_total_length(tracks: &[EngineTrack]) -> u64 {
    tracks
        .iter()
        .flat_map(|t| t.clips.iter())
        .map(|c| c.end_position())
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        track.clips.push(EngineClip {
            id: ClipId::new(),
            audio,
            position: 0,
            source_offset: 0,
            duration: 64,
        });

        let rendered = track.render(0, 8, 2);
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
        track.clips.push(EngineClip {
            id: ClipId::new(),
            audio: audio.clone(),
            position: 0,
            source_offset: 10,
            duration: 20,
        });

        let rendered = track.render(0, 4, 2);
        assert!(rendered);

        // First frame should come from source_offset 10
        let expected = audio.sample(0, 10);
        assert!((track.mix_buffer[0] - expected).abs() < 1e-6);
    }

    #[test]
    fn no_clips_returns_false() {
        let mut track = EngineTrack::new(TrackId::new());
        let rendered = track.render(0, 8, 2);
        assert!(!rendered);
    }

    #[test]
    fn total_length_calculation() {
        let audio = make_test_audio(100, 0.5);
        let mut tracks = vec![EngineTrack::new(TrackId::new())];
        tracks[0].clips.push(EngineClip {
            id: ClipId::new(),
            audio: audio.clone(),
            position: 50,
            source_offset: 0,
            duration: 100,
        });

        assert_eq!(calculate_total_length(&tracks), 150);
    }

    #[test]
    fn total_length_empty_tracks() {
        let tracks: Vec<EngineTrack> = vec![];
        assert_eq!(calculate_total_length(&tracks), 0);
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
        };
        // Requesting frames 30..60 — clip is at 40..90, overlaps
        assert!(clip.is_active(30, 30));
    }
}
