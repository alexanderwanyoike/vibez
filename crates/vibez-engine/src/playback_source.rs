//! Resident timeline content that feeds a shared engine channel strip.
//!
//! A playback source contains time-based musical content only. Instruments,
//! effects, sends, gain/pan, mute/solo, meters, and scratch buffers remain on
//! [`EngineTrack`](crate::mixer::EngineTrack), the project-owned channel strip.

use std::cell::Cell;
use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::automation::AutomationLane;
use vibez_core::id::{ClipId, SectionId, TrackId};
use vibez_core::midi::MidiNote;
use vibez_core::perform::GrooveGrid;

#[cfg(test)]
thread_local! {
    static AUDIO_THREAD_NOTE_END_LOOKUPS: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_audio_thread_note_end_lookups() {
    AUDIO_THREAD_NOTE_END_LOOKUPS.set(0);
}

#[cfg(test)]
pub(crate) fn audio_thread_note_end_lookups() -> usize {
    AUDIO_THREAD_NOTE_END_LOOKUPS.get()
}

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
    pub groove_grid: GrooveGrid,
    next_same_pitch_start_beats: Vec<Option<f64>>,
    groove_latch: Cell<Option<(i64, vibez_core::perform::SwingAmount)>>,
}

impl EngineNoteClip {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: ClipId,
        position_beats: f64,
        duration_beats: f64,
        notes: Vec<MidiNote>,
        loop_enabled: bool,
        loop_start_beats: f64,
        loop_end_beats: f64,
        groove_grid: GrooveGrid,
    ) -> Self {
        let next_same_pitch_start_beats = build_next_same_pitch_start_beats(&notes);
        Self {
            id,
            position_beats,
            duration_beats,
            notes,
            loop_enabled,
            loop_start_beats,
            loop_end_beats,
            groove_grid,
            next_same_pitch_start_beats,
            groove_latch: Cell::new(None),
        }
    }

    #[inline]
    pub(crate) fn next_same_pitch_start_beat(&self, note_index: usize) -> Option<f64> {
        #[cfg(test)]
        AUDIO_THREAD_NOTE_END_LOOKUPS.set(AUDIO_THREAD_NOTE_END_LOOKUPS.get() + 1);
        self.next_same_pitch_start_beats
            .get(note_index)
            .copied()
            .flatten()
    }

    pub(crate) fn push_note(&mut self, note: MidiNote) {
        let next_same_pitch_start = self
            .notes
            .iter()
            .filter(|existing| {
                existing.pitch == note.pitch && existing.start_beat > note.start_beat
            })
            .map(|existing| existing.start_beat)
            .min_by(f64::total_cmp);
        for (index, existing) in self.notes.iter().enumerate() {
            if existing.pitch == note.pitch && existing.start_beat < note.start_beat {
                let cached_next = &mut self.next_same_pitch_start_beats[index];
                if cached_next.is_none_or(|start| note.start_beat < start) {
                    *cached_next = Some(note.start_beat);
                }
            }
        }
        self.notes.push(note);
        self.next_same_pitch_start_beats.push(next_same_pitch_start);
    }

    pub(crate) fn remove_note(&mut self, note_index: usize) -> bool {
        if note_index >= self.notes.len() {
            return false;
        }
        self.notes.remove(note_index);
        self.rebuild_note_end_clamps();
        true
    }

    pub(crate) fn edit_note(&mut self, note_index: usize, note: MidiNote) -> bool {
        let Some(existing) = self.notes.get_mut(note_index) else {
            return false;
        };
        *existing = note;
        self.rebuild_note_end_clamps();
        true
    }

    fn rebuild_note_end_clamps(&mut self) {
        self.next_same_pitch_start_beats = build_next_same_pitch_start_beats(&self.notes);
    }

    pub fn reset_groove_latch(&self) {
        self.groove_latch.set(None);
    }

    pub fn swing_for_pair(
        &self,
        local_beat: f64,
        current: vibez_core::perform::SwingAmount,
        force_new_pair: bool,
    ) -> vibez_core::perform::SwingAmount {
        let Some(pair_beats) = self.groove_grid.pair_beats() else {
            return current;
        };
        let pair_index = (local_beat / pair_beats).floor() as i64;
        if !force_new_pair {
            if let Some((latched_pair, latched_swing)) = self.groove_latch.get() {
                if latched_pair == pair_index {
                    return latched_swing;
                }
            }
        }
        self.groove_latch.set(Some((pair_index, current)));
        current
    }
}

/// Precompute note-off clamps away from the realtime render loop. Notes retain
/// their caller-visible order; only the temporary index list is sorted.
fn build_next_same_pitch_start_beats(notes: &[MidiNote]) -> Vec<Option<f64>> {
    let mut by_descending_start: Vec<_> = (0..notes.len()).collect();
    by_descending_start
        .sort_by(|left, right| notes[*right].start_beat.total_cmp(&notes[*left].start_beat));
    let mut result = vec![None; notes.len()];
    let mut next_by_pitch = [None; 256];
    let mut group_start = 0;
    while group_start < by_descending_start.len() {
        let start_beat = notes[by_descending_start[group_start]].start_beat;
        let mut group_end = group_start + 1;
        while group_end < by_descending_start.len()
            && notes[by_descending_start[group_end]]
                .start_beat
                .total_cmp(&start_beat)
                .is_eq()
        {
            group_end += 1;
        }
        for &index in &by_descending_start[group_start..group_end] {
            result[index] = next_by_pitch[notes[index].pitch as usize];
        }
        for &index in &by_descending_start[group_start..group_end] {
            next_by_pitch[notes[index].pitch as usize] = Some(start_beat);
        }
        group_start = group_end;
    }
    result
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

/// One Project Track's resident content for a prepared Section.
pub struct PreparedTrackPlaybackSource {
    pub track_id: TrackId,
    pub source: Box<PreparedPlaybackSource>,
}

/// A complete, resident Section source prepared outside the audio callback.
///
/// The engine swaps each boxed track source in place. The same owner is then
/// returned through an engine event carrying the displaced sources, so no
/// source allocation or destruction occurs on the real-time thread.
pub struct PreparedSectionPlaybackSource {
    pub section_id: SectionId,
    pub length_beats: f64,
    pub looping: bool,
    tracks: Vec<PreparedTrackPlaybackSource>,
}

impl PreparedSectionPlaybackSource {
    pub fn new(
        section_id: SectionId,
        length_beats: f64,
        looping: bool,
        tracks: Vec<(TrackId, PreparedPlaybackSource)>,
    ) -> Self {
        Self {
            section_id,
            length_beats,
            looping,
            tracks: tracks
                .into_iter()
                .map(|(track_id, source)| PreparedTrackPlaybackSource {
                    track_id,
                    source: Box::new(source),
                })
                .collect(),
        }
    }

    pub fn tracks(&self) -> &[PreparedTrackPlaybackSource] {
        &self.tracks
    }

    pub(crate) fn tracks_mut(&mut self) -> &mut [PreparedTrackPlaybackSource] {
        &mut self.tracks
    }
}

impl std::fmt::Debug for PreparedSectionPlaybackSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedSectionPlaybackSource")
            .field("section_id", &self.section_id)
            .field("length_beats", &self.length_beats)
            .field("looping", &self.looping)
            .field("track_count", &self.tracks.len())
            .finish()
    }
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

    fn note(pitch: u8, start_beat: f64) -> MidiNote {
        MidiNote {
            pitch,
            velocity: 100,
            start_beat,
            duration_beats: 0.5,
        }
    }

    #[test]
    fn note_end_clamps_are_precomputed_without_reordering_notes() {
        let original = vec![
            note(42, 1.0),
            note(42, 0.5),
            note(43, 0.75),
            note(42, 1.5),
            note(42, 1.0),
        ];
        let mut clip = EngineNoteClip::new(
            ClipId::new(),
            0.0,
            4.0,
            original.clone(),
            false,
            0.0,
            0.0,
            GrooveGrid::Sixteenth,
        );

        assert_eq!(clip.notes, original);
        assert_eq!(clip.next_same_pitch_start_beat(0), Some(1.5));
        assert_eq!(clip.next_same_pitch_start_beat(1), Some(1.0));
        assert_eq!(clip.next_same_pitch_start_beat(2), None);
        assert_eq!(clip.next_same_pitch_start_beat(3), None);
        assert_eq!(clip.next_same_pitch_start_beat(4), Some(1.5));

        clip.push_note(note(42, 1.25));
        assert_eq!(clip.next_same_pitch_start_beat(0), Some(1.25));
        assert!(clip.edit_note(5, note(43, 1.25)));
        assert_eq!(clip.next_same_pitch_start_beat(0), Some(1.5));
        assert!(clip.remove_note(3));
        assert_eq!(clip.next_same_pitch_start_beat(0), None);
    }

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
            vec![EngineNoteClip::new(
                ClipId::new(),
                2.0,
                4.0,
                Vec::new(),
                false,
                0.0,
                0.0,
                GrooveGrid::Off,
            )],
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
