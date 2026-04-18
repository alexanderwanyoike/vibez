//! Deterministic MIDI phrase variation engine.
//!
//! Operates on [`NoteClipInfo`] to produce a set of mutated variants that keep
//! the identity of the source phrase intact but add musical motion. The seed
//! and mutator list determine the output completely: calling
//! [`generate_variants`] twice with the same seed returns the same output.
//!
//! The mutators are deliberately coarse: they're meant to be auditioned and
//! kept or thrown out, not tuned.

use serde::{Deserialize, Serialize};

use crate::id::ClipId;
use crate::midi::{MidiNote, NoteClipInfo};

/// Genre-tuned preset: an ordered list of mutators applied in sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GenrePreset {
    House,
    Techno,
    Trance,
    DnB,
    UKG,
    BigBeat,
    Electro,
}

impl GenrePreset {
    pub fn name(self) -> &'static str {
        match self {
            GenrePreset::House => "House",
            GenrePreset::Techno => "Techno",
            GenrePreset::Trance => "Trance",
            GenrePreset::DnB => "Drum & Bass",
            GenrePreset::UKG => "UK Garage",
            GenrePreset::BigBeat => "Big Beat",
            GenrePreset::Electro => "Electro",
        }
    }

    /// Pick a preset from a tempo. Intentionally biased toward the most
    /// common genre for each BPM range so "generate variations" does
    /// something musical by default.
    pub fn from_bpm(bpm: f64) -> GenrePreset {
        match bpm {
            b if b < 105.0 => GenrePreset::BigBeat,
            b if b < 122.0 => GenrePreset::House,
            b if b < 126.0 => GenrePreset::Electro,
            b if b < 132.0 => GenrePreset::Techno,
            b if b < 136.0 => GenrePreset::UKG,
            b if b < 150.0 => GenrePreset::Trance,
            _ => GenrePreset::DnB,
        }
    }

    fn mutators(self) -> &'static [PhraseMutator] {
        match self {
            GenrePreset::House => &[
                PhraseMutator::Swing { amount: 0.56 },
                PhraseMutator::GhostSnare {
                    snare_pitch: 38,
                    probability: 0.35,
                },
                PhraseMutator::Humanize {
                    timing_beats: 0.008,
                    velocity: 5,
                },
            ],
            GenrePreset::Techno => &[
                PhraseMutator::Thin { probability: 0.12 },
                PhraseMutator::Humanize {
                    timing_beats: 0.005,
                    velocity: 3,
                },
                PhraseMutator::EndFill,
            ],
            GenrePreset::Trance => &[
                PhraseMutator::Densify { factor: 0.25 },
                PhraseMutator::Humanize {
                    timing_beats: 0.01,
                    velocity: 4,
                },
                PhraseMutator::EndFill,
            ],
            GenrePreset::DnB => &[
                PhraseMutator::Swing { amount: 0.54 },
                PhraseMutator::GhostSnare {
                    snare_pitch: 38,
                    probability: 0.5,
                },
                PhraseMutator::EndFill,
                PhraseMutator::Humanize {
                    timing_beats: 0.005,
                    velocity: 3,
                },
            ],
            GenrePreset::UKG => &[
                PhraseMutator::Swing { amount: 0.65 },
                PhraseMutator::GhostSnare {
                    snare_pitch: 38,
                    probability: 0.4,
                },
                PhraseMutator::Humanize {
                    timing_beats: 0.012,
                    velocity: 6,
                },
            ],
            GenrePreset::BigBeat => &[
                PhraseMutator::Thin { probability: 0.08 },
                PhraseMutator::EndFill,
                PhraseMutator::Humanize {
                    timing_beats: 0.02,
                    velocity: 10,
                },
            ],
            GenrePreset::Electro => &[
                PhraseMutator::Densify { factor: 0.18 },
                PhraseMutator::Humanize {
                    timing_beats: 0.003,
                    velocity: 2,
                },
            ],
        }
    }
}

/// One step of a preset.
#[derive(Debug, Clone, Copy)]
pub enum PhraseMutator {
    /// Delay every odd 8th note by `(amount - 0.5) * 0.5` beats.
    /// Values above 0.5 produce swing; below returns straight time.
    Swing { amount: f32 },
    /// Drop notes with the given probability.
    Thin { probability: f32 },
    /// Insert copies of random notes on the adjacent 16th.
    Densify { factor: f32 },
    /// Insert low-velocity hits of `snare_pitch` between backbeats.
    GhostSnare { snare_pitch: u8, probability: f32 },
    /// Add a short fill pattern at the end of the phrase.
    EndFill,
    /// Random micro-jitter on timing and velocity.
    Humanize { timing_beats: f32, velocity: u8 },
}

impl PhraseMutator {
    fn apply(&self, notes: Vec<MidiNote>, phrase_beats: f64, rng: &mut Rng) -> Vec<MidiNote> {
        match *self {
            PhraseMutator::Swing { amount } => apply_swing(notes, amount),
            PhraseMutator::Thin { probability } => apply_thin(notes, probability, rng),
            PhraseMutator::Densify { factor } => apply_densify(notes, factor, phrase_beats, rng),
            PhraseMutator::GhostSnare {
                snare_pitch,
                probability,
            } => apply_ghost_snare(notes, snare_pitch, probability, phrase_beats, rng),
            PhraseMutator::EndFill => apply_end_fill(notes, phrase_beats, rng),
            PhraseMutator::Humanize {
                timing_beats,
                velocity,
            } => apply_humanize(notes, timing_beats, velocity, rng),
        }
    }
}

/// Generate `count` deterministic variants of `base` using `preset`.
///
/// Each variant is a fresh [`NoteClipInfo`] with a new `id`, inherits the
/// `track_id`, `position_beats`, `duration_beats`, and loop settings of the
/// source, and is named `"<source> v<i>"`.
pub fn generate_variants(
    base: &NoteClipInfo,
    preset: GenrePreset,
    seed_base: u64,
    count: usize,
) -> Vec<NoteClipInfo> {
    let mutators = preset.mutators();
    (0..count)
        .map(|i| {
            let seed = seed_base
                .wrapping_add((i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let mut rng = Rng::new(seed);
            let mut notes = base.notes.clone();
            for mutator in mutators {
                notes = mutator.apply(notes, base.duration_beats, &mut rng);
            }
            notes.sort_by(|a, b| a.start_beat.partial_cmp(&b.start_beat).unwrap_or(std::cmp::Ordering::Equal));
            NoteClipInfo {
                id: ClipId::new(),
                track_id: base.track_id,
                name: format!("{} v{}", base.name, i + 1),
                position_beats: base.position_beats,
                duration_beats: base.duration_beats,
                loop_enabled: base.loop_enabled,
                loop_start_beats: base.loop_start_beats,
                loop_end_beats: base.loop_end_beats,
                notes,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Mutator implementations
// ---------------------------------------------------------------------------

fn apply_swing(mut notes: Vec<MidiNote>, amount: f32) -> Vec<MidiNote> {
    let amount = amount.clamp(0.5, 0.75) as f64;
    if amount <= 0.5 {
        return notes;
    }
    let shift = (amount - 0.5) * 0.5;
    for note in notes.iter_mut() {
        let eighth_index = (note.start_beat * 2.0).round() as i64;
        if eighth_index.rem_euclid(2) == 1 {
            note.start_beat += shift;
        }
    }
    notes
}

fn apply_thin(notes: Vec<MidiNote>, probability: f32, rng: &mut Rng) -> Vec<MidiNote> {
    let p = probability.clamp(0.0, 1.0);
    notes
        .into_iter()
        .filter(|_| !rng.chance(p))
        .collect()
}

fn apply_densify(
    mut notes: Vec<MidiNote>,
    factor: f32,
    phrase_beats: f64,
    rng: &mut Rng,
) -> Vec<MidiNote> {
    let factor = factor.clamp(0.0, 1.0);
    if notes.is_empty() {
        return notes;
    }
    let original_len = notes.len();
    let extras = (original_len as f32 * factor).round() as usize;
    for _ in 0..extras {
        let idx = rng.next_u64() as usize % original_len;
        let mut clone = notes[idx];
        clone.start_beat = (clone.start_beat + 0.25).min(phrase_beats - 0.01);
        clone.velocity = clone.velocity.saturating_sub(20).max(30);
        clone.duration_beats = (clone.duration_beats * 0.5).max(0.05);
        notes.push(clone);
    }
    notes
}

fn apply_ghost_snare(
    mut notes: Vec<MidiNote>,
    snare_pitch: u8,
    probability: f32,
    phrase_beats: f64,
    rng: &mut Rng,
) -> Vec<MidiNote> {
    if phrase_beats <= 0.0 {
        return notes;
    }
    let p = probability.clamp(0.0, 1.0);
    let mut beat = 0.25;
    while beat < phrase_beats {
        if !is_backbeat(beat) && rng.chance(p) {
            notes.push(MidiNote {
                pitch: snare_pitch,
                velocity: 40 + (rng.next_u64() % 25) as u8,
                start_beat: beat,
                duration_beats: 0.12,
            });
        }
        beat += 0.5;
    }
    notes
}

fn is_backbeat(beat: f64) -> bool {
    let on_beat = (beat.fract().abs()) < 1e-3;
    let integer = beat.floor() as i64;
    on_beat && (integer.rem_euclid(4) == 1 || integer.rem_euclid(4) == 3)
}

fn apply_end_fill(
    mut notes: Vec<MidiNote>,
    phrase_beats: f64,
    rng: &mut Rng,
) -> Vec<MidiNote> {
    if phrase_beats < 2.0 {
        return notes;
    }
    let common_pitch = most_common_pitch(&notes).unwrap_or(60);
    let fill_start = phrase_beats - 0.5;
    for i in 0..4 {
        let offset = i as f64 * 0.125;
        let jitter = (rng.next_u64() % 8) as i64 - 4;
        notes.push(MidiNote {
            pitch: common_pitch,
            velocity: 70 + (rng.next_u64() % 40) as u8,
            start_beat: (fill_start + offset + jitter as f64 * 0.005).min(phrase_beats - 0.01),
            duration_beats: 0.1,
        });
    }
    notes
}

fn most_common_pitch(notes: &[MidiNote]) -> Option<u8> {
    let mut counts = [0usize; 128];
    for n in notes {
        counts[n.pitch as usize] += 1;
    }
    counts
        .iter()
        .enumerate()
        .max_by_key(|&(_, c)| *c)
        .filter(|(_, c)| **c > 0)
        .map(|(i, _)| i as u8)
}

fn apply_humanize(
    notes: Vec<MidiNote>,
    timing_beats: f32,
    velocity: u8,
    rng: &mut Rng,
) -> Vec<MidiNote> {
    let timing = timing_beats as f64;
    let vel_range = velocity as i32;
    notes
        .into_iter()
        .map(|mut n| {
            let t_jitter = rng.signed_unit() as f64 * timing;
            n.start_beat = (n.start_beat + t_jitter).max(0.0);
            if vel_range > 0 {
                let v = n.velocity as i32 + ((rng.signed_unit() * vel_range as f32).round() as i32);
                n.velocity = v.clamp(1, 127) as u8;
            }
            n
        })
        .collect()
}

// ---------------------------------------------------------------------------
// RNG
// ---------------------------------------------------------------------------

/// Deterministic xorshift64 RNG. Small, fast, and good enough for musical
/// noise; we don't need cryptographic quality here.
struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        // Avoid the zero state (xorshift is degenerate at 0).
        let seed = if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        };
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    fn next_unit(&mut self) -> f32 {
        ((self.next_u64() >> 32) as f32) / (u32::MAX as f32)
    }

    fn chance(&mut self, p: f32) -> bool {
        self.next_unit() < p
    }

    fn signed_unit(&mut self) -> f32 {
        self.next_unit() * 2.0 - 1.0
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::{ClipId, TrackId};

    fn sample_clip() -> NoteClipInfo {
        NoteClipInfo {
            id: ClipId::new(),
            track_id: TrackId::new(),
            name: "Pattern".into(),
            position_beats: 0.0,
            duration_beats: 4.0,
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
            notes: vec![
                MidiNote {
                    pitch: 36,
                    velocity: 100,
                    start_beat: 0.0,
                    duration_beats: 0.25,
                },
                MidiNote {
                    pitch: 36,
                    velocity: 100,
                    start_beat: 1.0,
                    duration_beats: 0.25,
                },
                MidiNote {
                    pitch: 38,
                    velocity: 95,
                    start_beat: 1.0,
                    duration_beats: 0.25,
                },
                MidiNote {
                    pitch: 36,
                    velocity: 100,
                    start_beat: 2.0,
                    duration_beats: 0.25,
                },
                MidiNote {
                    pitch: 38,
                    velocity: 95,
                    start_beat: 3.0,
                    duration_beats: 0.25,
                },
                MidiNote {
                    pitch: 42,
                    velocity: 60,
                    start_beat: 0.5,
                    duration_beats: 0.1,
                },
                MidiNote {
                    pitch: 42,
                    velocity: 60,
                    start_beat: 1.5,
                    duration_beats: 0.1,
                },
                MidiNote {
                    pitch: 42,
                    velocity: 60,
                    start_beat: 2.5,
                    duration_beats: 0.1,
                },
                MidiNote {
                    pitch: 42,
                    velocity: 60,
                    start_beat: 3.5,
                    duration_beats: 0.1,
                },
            ],
        }
    }

    #[test]
    fn determinism() {
        let clip = sample_clip();
        let a = generate_variants(&clip, GenrePreset::House, 42, 3);
        let b = generate_variants(&clip, GenrePreset::House, 42, 3);
        assert_eq!(a.len(), 3);
        for (va, vb) in a.iter().zip(b.iter()) {
            assert_eq!(va.notes.len(), vb.notes.len());
            for (na, nb) in va.notes.iter().zip(vb.notes.iter()) {
                assert_eq!(na.pitch, nb.pitch);
                assert!((na.start_beat - nb.start_beat).abs() < 1e-9);
                assert_eq!(na.velocity, nb.velocity);
            }
        }
    }

    #[test]
    fn each_variant_has_unique_id() {
        let clip = sample_clip();
        let variants = generate_variants(&clip, GenrePreset::Techno, 7, 4);
        let ids: std::collections::HashSet<_> = variants.iter().map(|v| v.id).collect();
        assert_eq!(ids.len(), variants.len());
        assert!(!ids.contains(&clip.id));
    }

    #[test]
    fn variants_preserve_frame() {
        let clip = sample_clip();
        let variants = generate_variants(&clip, GenrePreset::DnB, 1, 2);
        for v in &variants {
            assert_eq!(v.track_id, clip.track_id);
            assert!((v.position_beats - clip.position_beats).abs() < 1e-9);
            assert!((v.duration_beats - clip.duration_beats).abs() < 1e-9);
            assert_eq!(v.loop_enabled, clip.loop_enabled);
        }
    }

    #[test]
    fn variants_produce_musical_change() {
        let clip = sample_clip();
        let variants = generate_variants(&clip, GenrePreset::UKG, 99, 3);
        let base_sig: Vec<_> = clip
            .notes
            .iter()
            .map(|n| (n.pitch, (n.start_beat * 100.0).round() as i64))
            .collect();
        let any_changed = variants.iter().any(|v| {
            let sig: Vec<_> = v
                .notes
                .iter()
                .map(|n| (n.pitch, (n.start_beat * 100.0).round() as i64))
                .collect();
            sig != base_sig
        });
        assert!(any_changed);
    }

    #[test]
    fn preset_from_bpm() {
        assert_eq!(GenrePreset::from_bpm(100.0), GenrePreset::BigBeat);
        assert_eq!(GenrePreset::from_bpm(124.0), GenrePreset::Electro);
        assert_eq!(GenrePreset::from_bpm(130.0), GenrePreset::Techno);
        assert_eq!(GenrePreset::from_bpm(133.0), GenrePreset::UKG);
        assert_eq!(GenrePreset::from_bpm(138.0), GenrePreset::Trance);
        assert_eq!(GenrePreset::from_bpm(174.0), GenrePreset::DnB);
    }

    #[test]
    fn empty_input_stays_empty() {
        let mut clip = sample_clip();
        clip.notes.clear();
        let variants = generate_variants(&clip, GenrePreset::House, 0, 3);
        assert_eq!(variants.len(), 3);
    }

    #[test]
    fn thin_removes_some_notes() {
        let mut rng = Rng::new(42);
        let notes: Vec<_> = (0..100)
            .map(|i| MidiNote {
                pitch: 60,
                velocity: 100,
                start_beat: i as f64 * 0.1,
                duration_beats: 0.05,
            })
            .collect();
        let thinned = apply_thin(notes.clone(), 0.5, &mut rng);
        assert!(thinned.len() < notes.len());
        assert!(!thinned.is_empty());
    }

    #[test]
    fn swing_shifts_odd_eighths() {
        let notes = vec![
            MidiNote {
                pitch: 60,
                velocity: 100,
                start_beat: 0.0,
                duration_beats: 0.25,
            },
            MidiNote {
                pitch: 60,
                velocity: 100,
                start_beat: 0.5,
                duration_beats: 0.25,
            },
        ];
        let swung = apply_swing(notes, 0.6);
        assert!((swung[0].start_beat - 0.0).abs() < 1e-9);
        assert!(swung[1].start_beat > 0.5);
    }
}
