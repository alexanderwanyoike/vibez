//! Resident timeline content that feeds a shared engine channel strip.
//!
//! A playback source contains time-based musical content only. Instruments,
//! effects, sends, gain/pan, mute/solo, meters, and scratch buffers remain on
//! [`EngineTrack`](crate::mixer::EngineTrack), the project-owned channel strip.

use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::automation::AutomationLane;
use vibez_core::id::ClipId;
use vibez_core::midi::MidiNote;

/// A resident audio clip on a prepared timeline.
pub struct EngineClip {
    pub id: ClipId,
    pub audio: Arc<DecodedAudio>,
    pub position: u64,
    pub source_offset: u64,
    pub duration: u64,
    pub loop_enabled: bool,
    pub loop_start: u64,
    pub loop_end: u64,
}

impl EngineClip {
    pub fn end_position(&self) -> u64 {
        self.position.saturating_add(self.duration)
    }

    pub fn is_active(&self, pos: u64, frames: u64) -> bool {
        let end = pos.saturating_add(frames);
        self.position < end && self.end_position() > pos
    }
}

/// A resident MIDI note clip on a prepared timeline.
pub struct EngineNoteClip {
    pub id: ClipId,
    pub position_beats: f64,
    pub duration_beats: f64,
    pub notes: Vec<MidiNote>,
    pub loop_enabled: bool,
    pub loop_start_beats: f64,
    pub loop_end_beats: f64,
}

/// Map a raw timeline frame through the active Arrange loop.
#[inline]
fn apply_loop_wrap(global_frame: u64, loop_region: Option<(u64, u64)>) -> u64 {
    match loop_region {
        Some((start, end)) if end > start && global_frame >= end => {
            let loop_len = end - start;
            let overshoot = global_frame - end;
            start + (overshoot % loop_len)
        }
        _ => global_frame,
    }
}

/// Timeline content prepared before it is handed to the audio callback.
///
/// All audio clips already own decoded sample `Arc`s. The type deliberately
/// exposes no loading or I/O API: a later source switch can transfer one
/// prepared owner and swap only its pointer in the callback.
#[derive(Default)]
pub struct PreparedPlaybackSource {
    pub clips: Vec<EngineClip>,
    pub note_clips: Vec<EngineNoteClip>,
    pub automation: Vec<AutomationLane>,
}

impl PreparedPlaybackSource {
    pub fn new(
        clips: Vec<EngineClip>,
        note_clips: Vec<EngineNoteClip>,
        automation: Vec<AutomationLane>,
    ) -> Self {
        Self {
            clips,
            note_clips,
            automation,
        }
    }

    /// Render this resident source into a caller-owned channel buffer.
    /// Channel processing remains outside this type.
    pub fn render_audio(
        &self,
        output: &mut [f32],
        pos: u64,
        frames: usize,
        channels: usize,
        loop_region: Option<(u64, u64)>,
    ) -> bool {
        output.fill(0.0);
        let mut rendered_any = false;
        let block_crosses_loop = matches!(
            loop_region,
            Some((start, end)) if end > start
                && pos < end
                && pos.saturating_add(frames as u64) > end
        );

        for clip in &self.clips {
            if !block_crosses_loop && !clip.is_active(pos, frames as u64) {
                continue;
            }
            let audio_channels = clip.audio.num_channels();
            let mut clip_rendered = false;
            for frame in 0..frames {
                let global_frame = apply_loop_wrap(pos + frame as u64, loop_region);
                if global_frame < clip.position || global_frame >= clip.end_position() {
                    continue;
                }
                let clip_frame = (global_frame - clip.position) as usize;
                let source_frame = if clip.loop_enabled && clip.loop_end > clip.loop_start {
                    let raw = clip.source_offset as usize + clip_frame;
                    let loop_len = (clip.loop_end - clip.loop_start) as usize;
                    if raw >= clip.loop_end as usize {
                        clip.loop_start as usize + (raw - clip.loop_start as usize) % loop_len
                    } else {
                        raw
                    }
                } else {
                    clip.source_offset as usize + clip_frame
                };
                for ch in 0..channels {
                    let sample = if ch < audio_channels {
                        clip.audio.sample(ch, source_frame)
                    } else if audio_channels > 0 {
                        clip.audio.sample(audio_channels - 1, source_frame)
                    } else {
                        0.0
                    };
                    output[frame * channels + ch] += sample;
                }
                clip_rendered = true;
            }
            rendered_any |= clip_rendered;
        }
        rendered_any
    }
}

/// Calculate the total resident length across already-resolved sources.
pub fn calculate_total_length<'a, I>(sources: I, samples_per_beat: f64) -> u64
where
    I: Iterator<Item = &'a PreparedPlaybackSource> + Clone,
{
    let audio_end = sources
        .clone()
        .flat_map(|source| source.clips.iter())
        .map(EngineClip::end_position)
        .max()
        .unwrap_or(0);
    let note_end = if samples_per_beat.is_finite() && samples_per_beat > 0.0 {
        sources
            .flat_map(|source| source.note_clips.iter())
            .map(|clip| {
                ((clip.position_beats + clip.duration_beats) * samples_per_beat).round() as u64
            })
            .max()
            .unwrap_or(0)
    } else {
        0
    };
    audio_end.max(note_end)
}

/// Arrange's adapter into the engine playback-source boundary.
///
/// Live Arrange editing still mutates this resident source through the
/// existing lock-free command queue. Section playback supplies the second
/// adapter and pointer-switch behavior in Card 10.
pub struct ArrangementPlaybackSource;

impl ArrangementPlaybackSource {
    pub fn prepare_empty() -> PreparedPlaybackSource {
        PreparedPlaybackSource::default()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use vibez_core::audio_buffer::DecodedAudio;
    use vibez_core::id::{ClipId, TrackId};

    use super::*;
    use crate::mixer::EngineTrack;

    #[test]
    fn arrangement_adapter_prepares_an_empty_resident_source() {
        let source = ArrangementPlaybackSource::prepare_empty();
        assert!(source.clips.is_empty());
        assert!(source.note_clips.is_empty());
        assert!(source.automation.is_empty());
    }

    #[test]
    fn prepared_arrangement_source_renders_identically_through_the_channel_strip() {
        let audio = Arc::new(DecodedAudio {
            channels: vec![vec![0.25, -0.5, 0.75, -1.0]],
            sample_rate: 48_000,
        });
        let clip = || EngineClip {
            id: ClipId::new(),
            audio: Arc::clone(&audio),
            position: 0,
            source_offset: 0,
            duration: 4,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        };

        let mut existing_path = EngineTrack::new(TrackId::new());
        existing_path.playback_source.clips.push(clip());
        let mut prepared_path = EngineTrack::with_playback_source(
            TrackId::new(),
            PreparedPlaybackSource::new(vec![clip()], Vec::new(), Vec::new()),
        );

        assert!(existing_path.render(0, 4, 1, None));
        assert!(prepared_path.render(0, 4, 1, None));
        assert_eq!(existing_path.mix_buffer, prepared_path.mix_buffer);
    }

    #[test]
    fn total_length_combines_resident_audio_and_note_sources() {
        let audio = Arc::new(DecodedAudio {
            channels: vec![vec![0.5; 100]],
            sample_rate: 44_100,
        });
        let audio_source = PreparedPlaybackSource::new(
            vec![EngineClip {
                id: ClipId::new(),
                audio,
                position: 50,
                source_offset: 0,
                duration: 100,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
            }],
            Vec::new(),
            Vec::new(),
        );
        let note_source = PreparedPlaybackSource::new(
            Vec::new(),
            vec![EngineNoteClip {
                id: ClipId::new(),
                position_beats: 2.0,
                duration_beats: 4.0,
                notes: Vec::new(),
                loop_enabled: false,
                loop_start_beats: 0.0,
                loop_end_beats: 0.0,
            }],
            Vec::new(),
        );
        let sources = [audio_source, note_source];

        assert_eq!(calculate_total_length(sources.iter(), 22_050.0), 132_300);
        assert_eq!(
            calculate_total_length(std::iter::empty::<&PreparedPlaybackSource>(), 22_050.0),
            0
        );
    }
}
