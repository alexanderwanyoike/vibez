//! Shared event-read boundary for non-destructive MIDI clip Groove.

use super::*;
use vibez_core::midi::MidiNote;
use vibez_core::perform::GrooveGrid;

#[inline]
pub(super) fn mapped_note_start(clip: &EngineNoteClip, note: &MidiNote, swing: SwingAmount) -> f64 {
    clip.groove_grid.map_beat(note.start_beat, swing)
}

pub(super) fn mapped_note_end(clip: &EngineNoteClip, note_index: usize, swing: SwingAmount) -> f64 {
    let note = &clip.notes[note_index];
    let mapped_end = clip.groove_grid.map_beat(note.end_beat(), swing);
    if clip.groove_grid == GrooveGrid::Off {
        return mapped_end;
    }
    let next_same_pitch = clip
        .next_same_pitch_start_beat(note_index)
        .map(|start_beat| clip.groove_grid.map_beat(start_beat, swing));
    next_same_pitch.map_or(mapped_end, |next_start| mapped_end.min(next_start))
}

/// Schedule clip MIDI once per audio block instead of scanning every note for
/// every frame. Event beats are converted to the first sample whose timeline
/// position reaches them, preserving the old `previous < event <= current`
/// boundary and sample-accurate plugin offsets.
pub(super) fn collect_timed_note_events(
    clips: &[EngineNoteClip],
    pos: u64,
    frames: usize,
    samples_per_beat: f64,
    swing: SwingAmount,
    note_ons: &mut Vec<(u32, u8, u8)>,
    note_offs: &mut Vec<(u32, u8)>,
) {
    if frames == 0 || samples_per_beat <= 0.0 {
        return;
    }
    let last_sample = pos.saturating_add(frames as u64).saturating_sub(1);
    let previous_beat = if pos > 0 {
        (pos - 1) as f64 / samples_per_beat
    } else {
        -1.0
    };
    let first_beat = pos as f64 / samples_per_beat;
    let last_beat = last_sample as f64 / samples_per_beat;

    for clip in clips {
        let clip_start = clip.position_beats;
        let clip_end = clip.position_beats + clip.duration_beats;
        if clip_end <= previous_beat || clip_start > last_beat {
            continue;
        }

        let local_at_start = (first_beat - clip_start).max(0.0);
        let looping = clip.loop_enabled && clip.loop_end_beats > clip.loop_start_beats;
        let effective_at_start = if looping && local_at_start >= clip.loop_end_beats {
            let loop_len = clip.loop_end_beats - clip.loop_start_beats;
            clip.loop_start_beats + (local_at_start - clip.loop_start_beats) % loop_len
        } else {
            local_at_start
        };
        let entering_clip = previous_beat < clip_start;
        let clip_swing = clip.swing_for_pair(effective_at_start, swing, entering_clip);

        if looping {
            collect_looping_clip_events(
                clip,
                previous_beat - clip_start,
                last_beat - clip_start,
                pos,
                frames,
                samples_per_beat,
                clip_swing,
                note_ons,
                note_offs,
            );
        } else {
            for (note_index, note) in clip.notes.iter().enumerate() {
                let start = mapped_note_start(clip, note, clip_swing);
                if start < clip.duration_beats {
                    push_note_on(
                        clip_start + start,
                        note,
                        pos,
                        frames,
                        samples_per_beat,
                        note_ons,
                    );
                }
                let end = mapped_note_end(clip, note_index, clip_swing);
                if end < clip.duration_beats {
                    push_note_off(
                        clip_start + end,
                        note.pitch,
                        pos,
                        frames,
                        samples_per_beat,
                        note_offs,
                    );
                }
            }
        }

        // Leaving a clip kills anything still sounding, including notes whose
        // mapped end falls beyond the clip or loop boundary.
        if let Some(frame) = frame_for_beat(clip_end, pos, frames, samples_per_beat) {
            for note in &clip.notes {
                note_offs.push((frame, note.pitch));
            }
        }
    }

    note_ons.sort_by_key(|event| event.0);
    note_offs.sort_by_key(|event| event.0);
}

#[allow(clippy::too_many_arguments)]
fn collect_looping_clip_events(
    clip: &EngineNoteClip,
    previous_local: f64,
    last_local: f64,
    pos: u64,
    frames: usize,
    samples_per_beat: f64,
    swing: SwingAmount,
    note_ons: &mut Vec<(u32, u8, u8)>,
    note_offs: &mut Vec<(u32, u8)>,
) {
    let loop_start = clip.loop_start_beats;
    let loop_end = clip.loop_end_beats;
    let loop_len = loop_end - loop_start;

    for (note_index, note) in clip.notes.iter().enumerate() {
        let start = mapped_note_start(clip, note, swing);
        for_each_loop_occurrence(
            start,
            clip.duration_beats,
            loop_start,
            loop_end,
            loop_len,
            previous_local,
            last_local,
            |local| {
                push_note_on(
                    clip.position_beats + local,
                    note,
                    pos,
                    frames,
                    samples_per_beat,
                    note_ons,
                );
            },
        );

        let end = mapped_note_end(clip, note_index, swing);
        if end < loop_end {
            for_each_loop_occurrence(
                end,
                clip.duration_beats,
                loop_start,
                loop_end,
                loop_len,
                previous_local,
                last_local,
                |local| {
                    push_note_off(
                        clip.position_beats + local,
                        note.pitch,
                        pos,
                        frames,
                        samples_per_beat,
                        note_offs,
                    );
                },
            );
        }
    }

    // The previous renderer flushed all pitches at each loop wrap before
    // retriggering notes at loop_start on the same frame.
    for_each_repetition(
        loop_end,
        loop_len,
        clip.duration_beats,
        previous_local,
        last_local,
        |local| {
            if let Some(frame) =
                frame_for_beat(clip.position_beats + local, pos, frames, samples_per_beat)
            {
                for note in &clip.notes {
                    note_offs.push((frame, note.pitch));
                }
            }
        },
    );
}

#[allow(clippy::too_many_arguments)]
fn for_each_loop_occurrence(
    event: f64,
    clip_duration: f64,
    loop_start: f64,
    loop_end: f64,
    loop_len: f64,
    previous_local: f64,
    last_local: f64,
    mut visit: impl FnMut(f64),
) {
    if event < 0.0 || event >= clip_duration || event >= loop_end {
        return;
    }
    if event < loop_start {
        if previous_local < event && event <= last_local {
            visit(event);
        }
        return;
    }
    for_each_repetition(
        event,
        loop_len,
        clip_duration,
        previous_local,
        last_local,
        visit,
    );
}

fn for_each_repetition(
    first: f64,
    step: f64,
    clip_duration: f64,
    previous_local: f64,
    last_local: f64,
    mut visit: impl FnMut(f64),
) {
    let first_index = (((previous_local - first) / step).floor() as i64 + 1).max(0);
    let mut event = first + first_index as f64 * step;
    while event < clip_duration && event <= last_local {
        visit(event);
        event += step;
    }
}

fn push_note_on(
    beat: f64,
    note: &MidiNote,
    pos: u64,
    frames: usize,
    samples_per_beat: f64,
    note_ons: &mut Vec<(u32, u8, u8)>,
) {
    if let Some(frame) = frame_for_beat(beat, pos, frames, samples_per_beat) {
        note_ons.push((frame, note.pitch, note.velocity));
    }
}

fn push_note_off(
    beat: f64,
    pitch: u8,
    pos: u64,
    frames: usize,
    samples_per_beat: f64,
    note_offs: &mut Vec<(u32, u8)>,
) {
    if let Some(frame) = frame_for_beat(beat, pos, frames, samples_per_beat) {
        note_offs.push((frame, pitch));
    }
}

fn frame_for_beat(beat: f64, pos: u64, frames: usize, samples_per_beat: f64) -> Option<u32> {
    let sample = (beat * samples_per_beat).ceil();
    if !sample.is_finite() || sample < 0.0 {
        return None;
    }
    let sample = sample as u64;
    let end = pos.saturating_add(frames as u64);
    (sample >= pos && sample < end).then(|| (sample - pos) as u32)
}

impl EngineTrack {
    pub(super) fn reset_groove_latches(&self) {
        for clip in self
            .playback_source
            .note_clips
            .iter()
            .chain(self.section_playback_source.note_clips.iter())
        {
            clip.reset_groove_latch();
        }
    }
}
