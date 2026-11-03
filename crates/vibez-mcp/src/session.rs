//! The Vibez project an agent is authoring: tracks, devices and timeline content in musical units.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use vibez_core::effect::EffectInfo;
use vibez_core::id::{ClipId, EffectId, TrackId};
use vibez_core::midi::{MidiNote, NoteClipInfo, TrackKind};
use vibez_core::track::{ClipGainDb, ClipInfo, InstrumentStateInfo, MediaSourceRef, TrackInfo};
use vibez_project::Project;

use crate::device_params::{AppliedParameter, Device, DeviceParamError};

pub const MIN_BPM: f64 = 20.0;
pub const MAX_BPM: f64 = 400.0;

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("tempo must be between {MIN_BPM} and {MAX_BPM} BPM, got {0}")]
    InvalidTempo(f64),
    #[error("no track with id {0}")]
    UnknownTrack(u64),
    #[error("track {0} has no native instrument")]
    NotAnInstrumentTrack(u64),
    #[error("track {0} is an instrument track; audio clips need an audio track")]
    NotAnAudioTrack(u64),
    #[error("track {track} has no effect with id {effect}")]
    UnknownEffect { track: u64, effect: u64 },
    #[error("{0:?} is an effect, not an instrument")]
    NotAnInstrument(Device),
    #[error("{0:?} is an instrument, not an effect")]
    NotAnEffect(Device),
    #[error("the sampler needs a sample file")]
    SamplerWithoutSample,
    #[error("invalid timing: {0}")]
    InvalidTiming(String),
    #[error("invalid note: {0}")]
    InvalidNote(String),
    #[error("could not read sample {path}: {reason}")]
    Sample { path: PathBuf, reason: String },
    #[error(transparent)]
    DeviceParam(#[from] DeviceParamError),
}

/// A new track: an audio track when `instrument` is absent.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct NewTrack {
    pub name: String,
    #[serde(default)]
    pub instrument: Option<Device>,
    /// Sample played by a sampler instrument.
    #[serde(default)]
    pub sample: Option<PathBuf>,
    #[serde(default)]
    pub gain_db: Option<f32>,
    /// -1 is hard left, 0 centre, 1 hard right.
    #[serde(default)]
    pub pan: Option<f32>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct NewAudioClip {
    pub track_id: u64,
    pub sample: PathBuf,
    pub start_beat: f64,
    /// Defaults to the sample's own length.
    #[serde(default)]
    pub length_beats: Option<f64>,
    /// Loops the first `loop_beats` of the sample (at the project tempo) for the whole clip.
    #[serde(default)]
    pub loop_beats: Option<f64>,
    #[serde(default)]
    pub gain_db: Option<f32>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
pub struct Note {
    pub pitch: u8,
    pub velocity: u8,
    /// Relative to the clip start.
    pub start_beat: f64,
    pub length_beats: f64,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct NewNoteClip {
    pub track_id: u64,
    pub start_beat: f64,
    pub length_beats: f64,
    pub notes: Vec<Note>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct EffectSummary {
    pub id: u64,
    pub device: Device,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TrackSummary {
    pub id: u64,
    pub name: String,
    pub instrument: Option<Device>,
    pub gain_db: f32,
    pub pan: f32,
    pub mute: bool,
    pub effects: Vec<EffectSummary>,
    pub audio_clips: usize,
    pub note_clips: usize,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProjectSummary {
    pub name: String,
    pub bpm: f64,
    pub length_beats: f64,
    pub length_seconds: f64,
    pub saved_to: Option<PathBuf>,
    pub master: TrackSummary,
    pub tracks: Vec<TrackSummary>,
}

pub struct TrackSession {
    project: Project,
    container: Option<PathBuf>,
}

impl TrackSession {
    pub fn new(name: &str, bpm: f64) -> Result<Self, SessionError> {
        validate_tempo(bpm)?;
        let mut master = TrackInfo::new("Master");
        master.id = TrackId::MASTER;
        Ok(Self {
            project: Project {
                name: name.to_string(),
                bpm,
                master: Some(master),
                ..Project::default()
            },
            container: None,
        })
    }

    /// Adopts an existing project, e.g. one opened from disk.
    pub fn from_project(mut project: Project, container: Option<PathBuf>) -> Self {
        vibez_core::id::ensure_ids_above(project.max_persisted_id());
        if project.master.is_none() {
            let mut master = TrackInfo::new("Master");
            master.id = TrackId::MASTER;
            project.master = Some(master);
        }
        Self { project, container }
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    /// The `.vzp` this session was opened from or last saved to.
    pub fn container(&self) -> Option<&Path> {
        self.container.as_deref()
    }

    pub fn set_container(&mut self, path: PathBuf) {
        self.container = Some(path);
    }

    pub fn set_tempo(&mut self, bpm: f64) -> Result<(), SessionError> {
        validate_tempo(bpm)?;
        self.project.bpm = bpm;
        Ok(())
    }

    pub fn add_track(&mut self, spec: NewTrack) -> Result<u64, SessionError> {
        let mut track = TrackInfo::new(spec.name);
        track.color_index = (self.project.tracks.len() % 8) as u8;
        if let Some(device) = spec.instrument {
            let kind = device
                .instrument_kind()
                .ok_or(SessionError::NotAnInstrument(device))?;
            let params = device.default_params();
            let state = match device {
                Device::Sampler => {
                    let sample = spec.sample.ok_or(SessionError::SamplerWithoutSample)?;
                    sample_frames(&sample, self.project.sample_rate)?;
                    InstrumentStateInfo::Sampler {
                        params,
                        source: Some(MediaSourceRef::LocalFile { path: sample }),
                    }
                }
                Device::SubtractiveSynth => InstrumentStateInfo::SubtractiveSynth { params },
                Device::WavetableSynth => InstrumentStateInfo::WavetableSynth { params },
                _ => InstrumentStateInfo::FmSynth { params },
            };
            track.kind = TrackKind::Midi;
            track.instrument = Some(kind);
            track.native_instrument = Some(state);
        }
        if let Some(db) = spec.gain_db {
            track.gain = db_to_gain(db);
        }
        if let Some(pan) = spec.pan {
            track.pan = agent_pan_to_vibez(pan);
        }
        let id = track.id.raw();
        self.project.tracks.push(track);
        Ok(id)
    }

    pub fn set_track_mix(
        &mut self,
        track_id: u64,
        gain_db: Option<f32>,
        pan: Option<f32>,
        mute: Option<bool>,
    ) -> Result<(), SessionError> {
        let track = self.track_mut(track_id)?;
        if let Some(db) = gain_db {
            track.gain = db_to_gain(db);
        }
        if let Some(pan) = pan {
            track.pan = agent_pan_to_vibez(pan);
        }
        if let Some(mute) = mute {
            track.mute = mute;
        }
        Ok(())
    }

    pub fn set_instrument_parameters(
        &mut self,
        track_id: u64,
        values: &BTreeMap<String, f32>,
    ) -> Result<Vec<AppliedParameter>, SessionError> {
        let track = self.track_mut(track_id)?;
        let device = track
            .instrument
            .and_then(Device::for_instrument)
            .ok_or(SessionError::NotAnInstrumentTrack(track_id))?;
        let params = match track.native_instrument.as_mut() {
            Some(
                InstrumentStateInfo::SubtractiveSynth { params }
                | InstrumentStateInfo::WavetableSynth { params }
                | InstrumentStateInfo::FmSynth { params }
                | InstrumentStateInfo::Sampler { params, .. },
            ) => params,
            _ => return Err(SessionError::NotAnInstrumentTrack(track_id)),
        };
        Ok(crate::device_params::apply_named(device, params, values)?)
    }

    /// Appends an effect to a track's chain; track 0 is the master bus.
    pub fn add_effect(
        &mut self,
        track_id: u64,
        device: Device,
        values: &BTreeMap<String, f32>,
    ) -> Result<(u64, Vec<AppliedParameter>), SessionError> {
        let effect_type = device
            .effect_type()
            .ok_or(SessionError::NotAnEffect(device))?;
        let mut params = device.default_params();
        let applied = crate::device_params::apply_named(device, &mut params, values)?;
        let id = EffectId::new();
        self.track_mut(track_id)?.effects.push(EffectInfo {
            id,
            effect_type,
            bypass: false,
            params,
            plugin: None,
            sidechains: Vec::new(),
            inactive_sidechains: Vec::new(),
        });
        Ok((id.raw(), applied))
    }

    pub fn set_effect_parameters(
        &mut self,
        track_id: u64,
        effect_id: u64,
        values: &BTreeMap<String, f32>,
    ) -> Result<Vec<AppliedParameter>, SessionError> {
        let effect = self
            .track_mut(track_id)?
            .effects
            .iter_mut()
            .find(|effect| effect.id.raw() == effect_id)
            .ok_or(SessionError::UnknownEffect {
                track: track_id,
                effect: effect_id,
            })?;
        let device = Device::for_effect(effect.effect_type);
        Ok(crate::device_params::apply_named(
            device,
            &mut effect.params,
            values,
        )?)
    }

    pub fn add_audio_clip(&mut self, spec: NewAudioClip) -> Result<u64, SessionError> {
        validate_beat("start_beat", spec.start_beat)?;
        let track = self.track(spec.track_id)?;
        if track.instrument.is_some() {
            return Err(SessionError::NotAnAudioTrack(spec.track_id));
        }
        let track_id = track.id;
        let available = sample_frames(&spec.sample, self.project.sample_rate)?;
        let loop_end = match spec.loop_beats {
            Some(beats) => {
                validate_length("loop_beats", beats)?;
                let frames = self.beats_to_frames(beats);
                if frames > available {
                    return Err(SessionError::InvalidTiming(format!(
                        "a {beats}-beat loop needs {frames} frames but the sample has {available}"
                    )));
                }
                Some(frames)
            }
            None => None,
        };
        let duration = match spec.length_beats {
            Some(beats) => {
                validate_length("length_beats", beats)?;
                let frames = self.beats_to_frames(beats);
                if loop_end.is_none() && frames > available {
                    return Err(SessionError::InvalidTiming(format!(
                        "{beats} beats is longer than the sample; set loop_beats to repeat it"
                    )));
                }
                frames
            }
            None => available,
        };
        let gain_db = match spec.gain_db {
            Some(db) => ClipGainDb::new(db).ok_or_else(|| {
                SessionError::InvalidTiming(format!("clip gain {db} dB is out of range"))
            })?,
            None => ClipGainDb::default(),
        };
        let id = ClipId::new();
        let name = spec
            .sample
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Audio".into());
        self.project.arrange.clips.push(ClipInfo {
            id,
            track_id,
            name,
            position: self.beats_to_frames(spec.start_beat),
            source_offset: 0,
            start_marker: None,
            duration,
            source: Some(MediaSourceRef::LocalFile { path: spec.sample }),
            file_path: None,
            loop_enabled: loop_end.is_some(),
            loop_start: 0,
            loop_end: loop_end.unwrap_or(0),
            gain_db,
            fades: Default::default(),
            playback_direction: Default::default(),
            transient_markers: Default::default(),
            warp_markers: Default::default(),
            transpose: Default::default(),
            original_bpm: None,
            warped: false,
            warped_to_bpm: None,
        });
        Ok(id.raw())
    }

    pub fn add_note_clip(&mut self, spec: NewNoteClip) -> Result<u64, SessionError> {
        validate_beat("start_beat", spec.start_beat)?;
        validate_length("length_beats", spec.length_beats)?;
        let track = self.track(spec.track_id)?;
        if track.instrument.is_none() {
            return Err(SessionError::NotAnInstrumentTrack(spec.track_id));
        }
        let track_id = track.id;
        let notes = spec
            .notes
            .iter()
            .map(|note| validate_note(note, spec.length_beats))
            .collect::<Result<Vec<_>, _>>()?;
        let id = ClipId::new();
        self.project.arrange.note_clips.push(NoteClipInfo {
            id,
            track_id,
            name: format!("Notes {}", self.project.arrange.note_clips.len() + 1),
            position_beats: spec.start_beat,
            duration_beats: spec.length_beats,
            notes,
            start_marker_beats: None,
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
            groove_grid: Default::default(),
        });
        Ok(id.raw())
    }

    /// End of the last clip in beats.
    pub fn length_beats(&self) -> f64 {
        let audio_end = self
            .project
            .arrange
            .clips
            .iter()
            .map(|clip| self.frames_to_beats(clip.position + clip.duration))
            .fold(0.0, f64::max);
        let notes_end = self
            .project
            .arrange
            .note_clips
            .iter()
            .map(|clip| clip.position_beats + clip.duration_beats)
            .fold(0.0, f64::max);
        audio_end.max(notes_end)
    }

    pub fn beats_to_frames(&self, beats: f64) -> u64 {
        (beats * 60.0 / self.project.bpm * f64::from(self.project.sample_rate)).round() as u64
    }

    fn frames_to_beats(&self, frames: u64) -> f64 {
        frames as f64 / f64::from(self.project.sample_rate) * self.project.bpm / 60.0
    }

    pub fn summary(&self) -> ProjectSummary {
        let length_beats = self.length_beats();
        let master = self
            .project
            .master
            .as_ref()
            .map(|master| self.track_summary(master))
            .expect("a session always has a master");
        ProjectSummary {
            name: self.project.name.clone(),
            bpm: self.project.bpm,
            length_beats,
            length_seconds: length_beats * 60.0 / self.project.bpm,
            saved_to: self.container.clone(),
            master,
            tracks: self
                .project
                .tracks
                .iter()
                .map(|track| self.track_summary(track))
                .collect(),
        }
    }

    fn track_summary(&self, track: &TrackInfo) -> TrackSummary {
        let arrange = &self.project.arrange;
        TrackSummary {
            id: track.id.raw(),
            name: track.name.clone(),
            instrument: track.instrument.and_then(Device::for_instrument),
            gain_db: gain_to_db(track.gain),
            pan: track.pan * 2.0 - 1.0,
            mute: track.mute,
            effects: track
                .effects
                .iter()
                .map(|effect| EffectSummary {
                    id: effect.id.raw(),
                    device: Device::for_effect(effect.effect_type),
                })
                .collect(),
            audio_clips: arrange
                .clips
                .iter()
                .filter(|c| c.track_id == track.id)
                .count(),
            note_clips: arrange
                .note_clips
                .iter()
                .filter(|c| c.track_id == track.id)
                .count(),
        }
    }

    fn track(&self, track_id: u64) -> Result<&TrackInfo, SessionError> {
        self.project
            .tracks
            .iter()
            .find(|track| track.id.raw() == track_id)
            .ok_or(SessionError::UnknownTrack(track_id))
    }

    fn track_mut(&mut self, track_id: u64) -> Result<&mut TrackInfo, SessionError> {
        if track_id == TrackId::MASTER.raw() {
            return self
                .project
                .master
                .as_mut()
                .ok_or(SessionError::UnknownTrack(track_id));
        }
        self.project
            .tracks
            .iter_mut()
            .find(|track| track.id.raw() == track_id)
            .ok_or(SessionError::UnknownTrack(track_id))
    }
}

fn validate_tempo(bpm: f64) -> Result<(), SessionError> {
    if (MIN_BPM..=MAX_BPM).contains(&bpm) {
        Ok(())
    } else {
        Err(SessionError::InvalidTempo(bpm))
    }
}

fn validate_beat(name: &str, beats: f64) -> Result<(), SessionError> {
    if beats.is_finite() && beats >= 0.0 {
        Ok(())
    } else {
        Err(SessionError::InvalidTiming(format!(
            "{name} must be zero or more, got {beats}"
        )))
    }
}

fn validate_length(name: &str, beats: f64) -> Result<(), SessionError> {
    if beats.is_finite() && beats > 0.0 {
        Ok(())
    } else {
        Err(SessionError::InvalidTiming(format!(
            "{name} must be positive, got {beats}"
        )))
    }
}

fn validate_note(note: &Note, clip_beats: f64) -> Result<MidiNote, SessionError> {
    if note.pitch > 127 {
        return Err(SessionError::InvalidNote(format!(
            "pitch {} is above 127",
            note.pitch
        )));
    }
    if !(1..=127).contains(&note.velocity) {
        return Err(SessionError::InvalidNote(format!(
            "velocity {} must be 1 to 127",
            note.velocity
        )));
    }
    let fits = note.start_beat.is_finite()
        && note.length_beats.is_finite()
        && note.start_beat >= 0.0
        && note.length_beats > 0.0
        && note.start_beat < clip_beats;
    if !fits {
        return Err(SessionError::InvalidNote(format!(
            "note at beat {} for {} beats does not start inside a {clip_beats}-beat clip",
            note.start_beat, note.length_beats
        )));
    }
    Ok(MidiNote {
        pitch: note.pitch,
        velocity: note.velocity,
        start_beat: note.start_beat,
        duration_beats: note.length_beats,
    })
}

/// Length of a sample in project-rate frames, which is how clip geometry is stored.
fn sample_frames(path: &Path, project_rate: u32) -> Result<u64, SessionError> {
    let audio =
        vibez_audio_io::file_io::decode_audio_file(path).map_err(|error| SessionError::Sample {
            path: path.to_path_buf(),
            reason: error.to_string(),
        })?;
    let frames = audio.num_frames() as u64;
    Ok(frames * u64::from(project_rate) / u64::from(audio.sample_rate.max(1)))
}

fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

fn gain_to_db(gain: f32) -> f32 {
    20.0 * gain.max(1e-6).log10()
}

fn agent_pan_to_vibez(pan: f32) -> f32 {
    (pan.clamp(-1.0, 1.0) + 1.0) / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_core::audio_buffer::DecodedAudio;
    use vibez_core::track::InstrumentStateInfo;

    fn wav(dir: &Path, name: &str, seconds: f64) -> PathBuf {
        let path = dir.join(name);
        let frames = (seconds * 44_100.0) as usize;
        let audio = DecodedAudio {
            channels: vec![vec![0.25; frames]; 2],
            sample_rate: 44_100,
        };
        vibez_audio_io::file_io::write_wav_file(&path, &audio).unwrap();
        path
    }

    fn values(pairs: &[(&str, f32)]) -> BTreeMap<String, f32> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    fn session() -> TrackSession {
        TrackSession::new("test", 120.0).unwrap()
    }

    fn audio(name: &str) -> NewTrack {
        NewTrack {
            name: name.into(),
            instrument: None,
            sample: None,
            gain_db: None,
            pan: None,
        }
    }

    #[test]
    fn a_new_project_has_a_master_and_rejects_silly_tempos() {
        let s = session();
        assert_eq!(s.project().bpm, 120.0);
        assert!(s.project().master.is_some());
        assert!(matches!(
            TrackSession::new("x", 5.0),
            Err(SessionError::InvalidTempo(_))
        ));
    }

    #[test]
    fn instrument_tracks_carry_native_state_with_vibez_defaults() {
        let mut s = session();
        let id = s
            .add_track(NewTrack {
                instrument: Some(Device::FmSynth),
                ..audio("keys")
            })
            .unwrap();
        let track = s
            .project()
            .tracks
            .iter()
            .find(|t| t.id.raw() == id)
            .unwrap();
        match track.native_instrument.as_ref().unwrap() {
            InstrumentStateInfo::FmSynth { params } => {
                assert_eq!(params.len(), Device::FmSynth.descriptors().len())
            }
            other => panic!("unexpected instrument {other:?}"),
        }
    }

    #[test]
    fn a_sampler_requires_and_references_its_sample() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = session();
        assert!(matches!(
            s.add_track(NewTrack {
                instrument: Some(Device::Sampler),
                ..audio("bass")
            }),
            Err(SessionError::SamplerWithoutSample)
        ));
        let id = s
            .add_track(NewTrack {
                instrument: Some(Device::Sampler),
                sample: Some(wav(dir.path(), "bass.wav", 0.5)),
                ..audio("bass")
            })
            .unwrap();
        let applied = s
            .set_instrument_parameters(id, &values(&[("Root Note", 36.0)]))
            .unwrap();
        assert_eq!(applied[0].applied, 36.0);
    }

    #[test]
    fn mix_values_convert_from_agent_units() {
        let mut s = session();
        let id = s.add_track(audio("drums")).unwrap();
        s.set_track_mix(id, Some(-6.0), Some(-1.0), Some(true))
            .unwrap();
        let track = &s.project().tracks[0];
        assert!((track.gain - 0.501).abs() < 0.001);
        assert_eq!(track.pan, 0.0);
        assert!(track.mute);
    }

    #[test]
    fn effects_chain_on_tracks_and_master() {
        let mut s = session();
        let id = s.add_track(audio("drums")).unwrap();
        let (effect, _) = s
            .add_effect(id, Device::Reverb, &values(&[("Mix", 0.4)]))
            .unwrap();
        s.set_effect_parameters(id, effect, &values(&[("Room Size", 0.9)]))
            .unwrap();
        s.add_effect(0, Device::Limiter, &BTreeMap::new()).unwrap();
        assert_eq!(s.project().tracks[0].effects[0].params[0], 0.9);
        assert_eq!(s.project().master.as_ref().unwrap().effects.len(), 1);
        assert!(matches!(
            s.add_effect(id, Device::FmSynth, &BTreeMap::new()),
            Err(SessionError::NotAnEffect(_))
        ));
    }

    #[test]
    fn looping_audio_clips_are_measured_at_the_project_tempo() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = session();
        let id = s.add_track(audio("break")).unwrap();
        let sample = wav(dir.path(), "break.wav", 2.2);
        s.add_audio_clip(NewAudioClip {
            track_id: id,
            sample,
            start_beat: 16.0,
            length_beats: Some(32.0),
            loop_beats: Some(4.0),
            gain_db: None,
        })
        .unwrap();
        let clip = &s.project().arrange.clips[0];
        assert!(clip.loop_enabled);
        assert_eq!(clip.position, s.beats_to_frames(16.0));
        assert_eq!(clip.loop_end, s.beats_to_frames(4.0));
        assert_eq!(clip.duration, s.beats_to_frames(32.0));
        assert_eq!(s.length_beats(), 48.0);
    }

    #[test]
    fn a_loop_cannot_reach_past_the_end_of_its_sample() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = session();
        let id = s.add_track(audio("break")).unwrap();
        let result = s.add_audio_clip(NewAudioClip {
            track_id: id,
            sample: wav(dir.path(), "short.wav", 1.0),
            start_beat: 0.0,
            length_beats: Some(8.0),
            loop_beats: Some(4.0),
            gain_db: None,
        });
        assert!(matches!(result, Err(SessionError::InvalidTiming(_))));
    }

    #[test]
    fn note_clips_validate_their_notes() {
        let mut s = session();
        let keys = s
            .add_track(NewTrack {
                instrument: Some(Device::SubtractiveSynth),
                ..audio("keys")
            })
            .unwrap();
        let note = Note {
            pitch: 60,
            velocity: 90,
            start_beat: 0.0,
            length_beats: 4.0,
        };
        s.add_note_clip(NewNoteClip {
            track_id: keys,
            start_beat: 8.0,
            length_beats: 8.0,
            notes: vec![note],
        })
        .unwrap();
        assert_eq!(s.project().arrange.note_clips[0].notes[0].pitch, 60);
        let bad = s.add_note_clip(NewNoteClip {
            track_id: keys,
            start_beat: 0.0,
            length_beats: 4.0,
            notes: vec![Note { pitch: 200, ..note }],
        });
        assert!(matches!(bad, Err(SessionError::InvalidNote(_))));
        let audio_track = s.add_track(audio("vox")).unwrap();
        assert!(matches!(
            s.add_note_clip(NewNoteClip {
                track_id: audio_track,
                start_beat: 0.0,
                length_beats: 4.0,
                notes: vec![note],
            }),
            Err(SessionError::NotAnInstrumentTrack(_))
        ));
    }

    #[test]
    fn the_summary_reports_tracks_devices_and_length() {
        let mut s = session();
        let id = s.add_track(audio("drums")).unwrap();
        s.add_effect(id, Device::Eq, &BTreeMap::new()).unwrap();
        let summary = s.summary();
        assert_eq!(summary.tracks.len(), 1);
        assert_eq!(summary.tracks[0].effects[0].device, Device::Eq);
        assert_eq!(summary.bpm, 120.0);
    }
}
