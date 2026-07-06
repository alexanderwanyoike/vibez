//! Offline rendering (bounce / resample).
//!
//! Builds a fresh set of [`EngineTrack`]s from a project snapshot, drives them
//! block-by-block without an audio I/O stream, and returns a
//! [`DecodedAudio`] buffer. The same mixer and instrument paths that run on
//! the audio thread are used, so what you bounce matches what you hear.
//!
//! Plugin instruments and plugin effects are **not** reconstructed offline in
//! this version: tracks or effect slots backed by an external plugin are
//! silently skipped and a warning is emitted. Native instruments and native
//! effects (`vibez-dsp`) render fully.

use std::collections::HashMap;
use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::NoteClipInfo;
use vibez_core::time::TempoMap;
use vibez_core::track::{ClipInfo, InstrumentStateInfo, TrackInfo};
use vibez_dsp::factory::create_effect_with_params;
use vibez_instruments::create_instrument;

use crate::mixer::{
    any_solo, equal_power_pan, EffectSlot, EngineClip, EngineNoteClip, EngineTrack,
};

/// What to render offline.
#[derive(Debug, Clone, Copy)]
pub enum BounceMode {
    /// Sum of all non-muted / soloed tracks. Mute and solo rules mirror live
    /// playback.
    Master,
    /// Render one track post-effects with its gain/pan. Mute/solo on the
    /// target are ignored so you always hear the thing you asked for.
    Track(TrackId),
    /// Render a single clip on one track. Other clips on the same track and
    /// all other tracks are excluded.
    Clip {
        track_id: TrackId,
        clip_id: ClipId,
        is_note_clip: bool,
    },
}

/// Snapshot of the parts of a project needed to render offline, plus the
/// decoded audio for every asset referenced by the snapshot.
pub struct BounceRequest {
    pub tracks: Vec<TrackInfo>,
    pub audio_clips: Vec<ClipInfo>,
    pub note_clips: Vec<NoteClipInfo>,
    pub clip_audio: HashMap<ClipId, Arc<DecodedAudio>>,
    pub sampler_audio: HashMap<TrackId, (Arc<DecodedAudio>, String)>,
    pub drum_pad_audio: HashMap<(TrackId, usize), (Arc<DecodedAudio>, String)>,
    pub mode: BounceMode,
    /// Render window in absolute samples: `[start, end)` at `sample_rate`.
    pub range_samples: (u64, u64),
    pub bpm: f64,
    pub sample_rate: u32,
}

pub struct BounceResult {
    pub audio: DecodedAudio,
    pub warnings: Vec<String>,
}

const BLOCK_FRAMES: usize = 512;
const CHANNELS: usize = 2;

/// Render the request and return interleaved stereo [`DecodedAudio`] at the
/// project's sample rate.
pub fn render_offline(req: &BounceRequest) -> BounceResult {
    let mut warnings = Vec::new();
    let sr_f32 = req.sample_rate as f32;
    let tempo = TempoMap::new(req.bpm, req.sample_rate);

    let mut tracks: Vec<EngineTrack> = Vec::with_capacity(req.tracks.len());

    for track_info in &req.tracks {
        if !track_is_active_for_mode(track_info.id, req.mode) {
            continue;
        }

        let mut engine = EngineTrack::new(track_info.id);
        engine.gain = track_info.gain;
        engine.pan = track_info.pan;
        match req.mode {
            BounceMode::Master => {
                engine.mute = track_info.mute;
                engine.solo = track_info.solo;
            }
            _ => {
                engine.mute = false;
                engine.solo = false;
            }
        }

        if let Some(kind) = track_info.instrument {
            let mut instrument = create_instrument(kind, sr_f32);
            if let Some(state) = &track_info.native_instrument {
                match state {
                    InstrumentStateInfo::SubtractiveSynth { params } => {
                        for (i, v) in params.iter().enumerate() {
                            instrument.set_param(i, *v);
                        }
                    }
                    InstrumentStateInfo::Sampler { params, .. } => {
                        for (i, v) in params.iter().enumerate() {
                            instrument.set_param(i, *v);
                        }
                        if let Some((audio, name)) = req.sampler_audio.get(&track_info.id) {
                            instrument.load_sample(Arc::clone(audio), name.clone());
                        }
                    }
                    InstrumentStateInfo::DrumRack { pads } => {
                        for (idx, pad) in pads.iter().enumerate() {
                            instrument.set_drum_pad_state(idx, pad.clone());
                        }
                        for (idx, _) in pads.iter().enumerate() {
                            if let Some((audio, name)) =
                                req.drum_pad_audio.get(&(track_info.id, idx))
                            {
                                instrument.load_drum_pad_sample(
                                    idx,
                                    Arc::clone(audio),
                                    name.clone(),
                                );
                            }
                        }
                    }
                }
            }
            engine.instrument = Some(instrument);
        } else if track_info.kind.is_midi() {
            warnings.push(format!(
                "Track '{}' has no native instrument; any plugin instrument will not render",
                track_info.name
            ));
        }

        for info in &track_info.effects {
            let fx = create_effect_with_params(info.effect_type, sr_f32, &info.params);
            engine.effects.push(EffectSlot {
                id: info.id,
                effect: fx,
                bypass: info.bypass,
            });
        }

        for clip in req
            .audio_clips
            .iter()
            .filter(|c| c.track_id == track_info.id)
        {
            if !clip_included_for_mode(clip.id, req.mode, false) {
                continue;
            }
            match req.clip_audio.get(&clip.id) {
                Some(audio) => engine.clips.push(EngineClip {
                    id: clip.id,
                    audio: Arc::clone(audio),
                    position: clip.position,
                    source_offset: clip.source_offset,
                    duration: clip.duration,
                    loop_enabled: clip.loop_enabled,
                    loop_start: clip.loop_start,
                    loop_end: clip.loop_end,
                }),
                None => warnings.push(format!("Clip '{}' audio missing, skipped", clip.name)),
            }
        }

        for nc in req
            .note_clips
            .iter()
            .filter(|c| c.track_id == track_info.id)
        {
            if !clip_included_for_mode(nc.id, req.mode, true) {
                continue;
            }
            engine.note_clips.push(EngineNoteClip {
                id: nc.id,
                position_beats: nc.position_beats,
                duration_beats: nc.duration_beats,
                notes: nc.notes.clone(),
                loop_enabled: nc.loop_enabled,
                loop_start_beats: nc.loop_start_beats,
                loop_end_beats: nc.loop_end_beats,
            });
        }

        tracks.push(engine);
    }

    let (start, end) = req.range_samples;
    let total_frames = end.saturating_sub(start) as usize;
    let has_solo = matches!(req.mode, BounceMode::Master) && any_solo(&tracks);

    let mut out_l = Vec::with_capacity(total_frames);
    let mut out_r = Vec::with_capacity(total_frames);

    let mut master_scratch = vec![0.0f32; BLOCK_FRAMES * CHANNELS];

    let mut rendered = 0usize;
    while rendered < total_frames {
        let block = (total_frames - rendered).min(BLOCK_FRAMES);
        let pos = start + rendered as u64;
        let scratch = &mut master_scratch[..block * CHANNELS];
        scratch.iter_mut().for_each(|s| *s = 0.0);

        for track in tracks.iter_mut() {
            if matches!(req.mode, BounceMode::Master) {
                if track.mute {
                    continue;
                }
                if has_solo && !track.solo {
                    continue;
                }
            }

            let produced = if track.instrument.is_some() {
                track.render_instrument(pos, block, CHANNELS, &tempo)
            } else {
                // Offline bounce never loops the arrangement — it
                // walks the timeline linearly from `range.0` to
                // `range.1`, so pass `None`.
                track.render(pos, block, CHANNELS, None)
            };
            if !produced {
                continue;
            }
            track.process_effects(block, CHANNELS);

            let (pan_l, pan_r) = equal_power_pan(track.pan);
            let gain = track.gain;
            for frame in 0..block {
                let idx = frame * CHANNELS;
                scratch[idx] += track.mix_buffer[idx] * gain * pan_l;
                scratch[idx + 1] += track.mix_buffer[idx + 1] * gain * pan_r;
            }
        }

        for frame in 0..block {
            let idx = frame * CHANNELS;
            out_l.push(scratch[idx]);
            out_r.push(scratch[idx + 1]);
        }

        rendered += block;
    }

    BounceResult {
        audio: DecodedAudio {
            channels: vec![out_l, out_r],
            sample_rate: req.sample_rate,
        },
        warnings,
    }
}

fn track_is_active_for_mode(track_id: TrackId, mode: BounceMode) -> bool {
    match mode {
        BounceMode::Master => true,
        BounceMode::Track(tid) => tid == track_id,
        BounceMode::Clip { track_id: tid, .. } => tid == track_id,
    }
}

fn clip_included_for_mode(clip_id: ClipId, mode: BounceMode, is_note_clip: bool) -> bool {
    match mode {
        BounceMode::Master | BounceMode::Track(_) => true,
        BounceMode::Clip {
            clip_id: target,
            is_note_clip: target_is_note,
            ..
        } => target == clip_id && target_is_note == is_note_clip,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_core::constants::{DEFAULT_TRACK_GAIN, DEFAULT_TRACK_PAN};
    use vibez_core::effect::{EffectInfo, EffectType};
    use vibez_core::id::EffectId;
    use vibez_core::midi::{InstrumentKind, MidiNote, TrackKind};

    fn audio_of(frames: usize, value: f32) -> Arc<DecodedAudio> {
        Arc::new(DecodedAudio {
            channels: vec![vec![value; frames], vec![value; frames]],
            sample_rate: 44_100,
        })
    }

    fn bare_track(name: &str) -> TrackInfo {
        TrackInfo {
            id: TrackId::new(),
            name: name.into(),
            gain: DEFAULT_TRACK_GAIN,
            pan: DEFAULT_TRACK_PAN,
            mute: false,
            solo: false,
            effects: Vec::new(),
            kind: TrackKind::Audio,
            color_index: 0,
            instrument: None,
            native_instrument: None,
            plugin_instrument: None,
        }
    }

    #[test]
    fn master_renders_single_clip() {
        let mut track = bare_track("audio");
        track.pan = DEFAULT_TRACK_PAN;
        let tid = track.id;
        let audio = audio_of(200, 0.5);
        let cid = ClipId::new();
        let clip = ClipInfo {
            id: cid,
            track_id: tid,
            name: "c".into(),
            position: 0,
            source_offset: 0,
            duration: 200,
            source: None,
            file_path: None,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            original_bpm: None,
            warped: false,
            warped_to_bpm: None,
        };

        let mut clip_audio = HashMap::new();
        clip_audio.insert(cid, audio);

        let req = BounceRequest {
            tracks: vec![track],
            audio_clips: vec![clip],
            note_clips: Vec::new(),
            clip_audio,
            sampler_audio: HashMap::new(),
            drum_pad_audio: HashMap::new(),
            mode: BounceMode::Master,
            range_samples: (0, 200),
            bpm: 120.0,
            sample_rate: 44_100,
        };

        let result = render_offline(&req);
        assert_eq!(result.audio.num_frames(), 200);
        let expected = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
        for frame in 0..200 {
            assert!((result.audio.channels[0][frame] - expected).abs() < 1e-4);
            assert!((result.audio.channels[1][frame] - expected).abs() < 1e-4);
        }
    }

    #[test]
    fn mute_silences_master_bounce() {
        let mut track = bare_track("audio");
        track.mute = true;
        let tid = track.id;
        let audio = audio_of(100, 0.7);
        let cid = ClipId::new();
        let clip = ClipInfo {
            id: cid,
            track_id: tid,
            name: "c".into(),
            position: 0,
            source_offset: 0,
            duration: 100,
            source: None,
            file_path: None,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            original_bpm: None,
            warped: false,
            warped_to_bpm: None,
        };
        let mut clip_audio = HashMap::new();
        clip_audio.insert(cid, audio);

        let req = BounceRequest {
            tracks: vec![track],
            audio_clips: vec![clip],
            note_clips: Vec::new(),
            clip_audio,
            sampler_audio: HashMap::new(),
            drum_pad_audio: HashMap::new(),
            mode: BounceMode::Master,
            range_samples: (0, 100),
            bpm: 120.0,
            sample_rate: 44_100,
        };
        let out = render_offline(&req);
        assert!(out.audio.channels[0].iter().all(|&s| s.abs() < 1e-6));
    }

    #[test]
    fn track_mode_ignores_mute() {
        let mut track = bare_track("audio");
        track.mute = true;
        let tid = track.id;
        let audio = audio_of(100, 0.5);
        let cid = ClipId::new();
        let clip = ClipInfo {
            id: cid,
            track_id: tid,
            name: "c".into(),
            position: 0,
            source_offset: 0,
            duration: 100,
            source: None,
            file_path: None,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            original_bpm: None,
            warped: false,
            warped_to_bpm: None,
        };
        let mut clip_audio = HashMap::new();
        clip_audio.insert(cid, audio);
        let req = BounceRequest {
            tracks: vec![track],
            audio_clips: vec![clip],
            note_clips: Vec::new(),
            clip_audio,
            sampler_audio: HashMap::new(),
            drum_pad_audio: HashMap::new(),
            mode: BounceMode::Track(tid),
            range_samples: (0, 100),
            bpm: 120.0,
            sample_rate: 44_100,
        };
        let out = render_offline(&req);
        assert!(out.audio.channels[0].iter().any(|&s| s.abs() > 1e-4));
    }

    #[test]
    fn clip_mode_isolates_single_clip() {
        let mut track = bare_track("audio");
        let tid = track.id;
        track.pan = DEFAULT_TRACK_PAN;
        let cid_a = ClipId::new();
        let cid_b = ClipId::new();
        let audio_a = audio_of(100, 0.3);
        let audio_b = audio_of(100, 0.9);
        let clip_a = ClipInfo {
            id: cid_a,
            track_id: tid,
            name: "a".into(),
            position: 0,
            source_offset: 0,
            duration: 100,
            source: None,
            file_path: None,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            original_bpm: None,
            warped: false,
            warped_to_bpm: None,
        };
        let clip_b = ClipInfo {
            id: cid_b,
            track_id: tid,
            name: "b".into(),
            position: 0,
            source_offset: 0,
            duration: 100,
            source: None,
            file_path: None,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            original_bpm: None,
            warped: false,
            warped_to_bpm: None,
        };
        let mut clip_audio = HashMap::new();
        clip_audio.insert(cid_a, audio_a);
        clip_audio.insert(cid_b, audio_b);

        let req = BounceRequest {
            tracks: vec![track],
            audio_clips: vec![clip_a, clip_b],
            note_clips: Vec::new(),
            clip_audio,
            sampler_audio: HashMap::new(),
            drum_pad_audio: HashMap::new(),
            mode: BounceMode::Clip {
                track_id: tid,
                clip_id: cid_a,
                is_note_clip: false,
            },
            range_samples: (0, 100),
            bpm: 120.0,
            sample_rate: 44_100,
        };
        let out = render_offline(&req);
        let expected = 0.3 * std::f32::consts::FRAC_1_SQRT_2;
        assert!((out.audio.channels[0][0] - expected).abs() < 1e-4);
    }

    #[test]
    fn synth_note_clip_produces_audio() {
        let tid = TrackId::new();
        let cid = ClipId::new();
        let track = TrackInfo {
            id: tid,
            name: "Synth".into(),
            gain: DEFAULT_TRACK_GAIN,
            pan: DEFAULT_TRACK_PAN,
            mute: false,
            solo: false,
            effects: Vec::new(),
            kind: TrackKind::Instrument(InstrumentKind::SubtractiveSynth),
            color_index: 0,
            instrument: Some(InstrumentKind::SubtractiveSynth),
            native_instrument: Some(InstrumentStateInfo::SubtractiveSynth { params: Vec::new() }),
            plugin_instrument: None,
        };
        let note_clip = NoteClipInfo {
            id: cid,
            track_id: tid,
            name: "p".into(),
            position_beats: 0.0,
            duration_beats: 1.0,
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
            notes: vec![MidiNote {
                pitch: 60,
                velocity: 100,
                start_beat: 0.0,
                duration_beats: 0.5,
            }],
        };
        let req = BounceRequest {
            tracks: vec![track],
            audio_clips: Vec::new(),
            note_clips: vec![note_clip],
            clip_audio: HashMap::new(),
            sampler_audio: HashMap::new(),
            drum_pad_audio: HashMap::new(),
            mode: BounceMode::Track(tid),
            range_samples: (0, 22_050),
            bpm: 120.0,
            sample_rate: 44_100,
        };
        let out = render_offline(&req);
        assert!(out.audio.channels[0].iter().any(|&s| s.abs() > 1e-3));
    }

    #[test]
    fn warns_on_midi_track_with_no_native_instrument() {
        let tid = TrackId::new();
        let track = TrackInfo {
            id: tid,
            name: "Plugin Stub".into(),
            gain: DEFAULT_TRACK_GAIN,
            pan: DEFAULT_TRACK_PAN,
            mute: false,
            solo: false,
            effects: Vec::new(),
            kind: TrackKind::Midi,
            color_index: 0,
            instrument: None,
            native_instrument: None,
            plugin_instrument: None,
        };
        let req = BounceRequest {
            tracks: vec![track],
            audio_clips: Vec::new(),
            note_clips: Vec::new(),
            clip_audio: HashMap::new(),
            sampler_audio: HashMap::new(),
            drum_pad_audio: HashMap::new(),
            mode: BounceMode::Track(tid),
            range_samples: (0, 4_410),
            bpm: 120.0,
            sample_rate: 44_100,
        };
        let out = render_offline(&req);
        assert!(!out.warnings.is_empty());
    }

    #[test]
    fn effect_chain_applied_during_bounce() {
        let tid = TrackId::new();
        let cid = ClipId::new();
        let mut track = bare_track("audio");
        track.id = tid;
        // Gain of 0.5 halves the bounce output
        track.effects.push(EffectInfo {
            id: EffectId::new(),
            effect_type: EffectType::Gain,
            bypass: false,
            params: vec![0.5],
            plugin: None,
        });
        let audio = audio_of(100, 1.0);
        let clip = ClipInfo {
            id: cid,
            track_id: tid,
            name: "c".into(),
            position: 0,
            source_offset: 0,
            duration: 100,
            source: None,
            file_path: None,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            original_bpm: None,
            warped: false,
            warped_to_bpm: None,
        };
        let mut clip_audio = HashMap::new();
        clip_audio.insert(cid, audio);

        let req = BounceRequest {
            tracks: vec![track],
            audio_clips: vec![clip],
            note_clips: Vec::new(),
            clip_audio,
            sampler_audio: HashMap::new(),
            drum_pad_audio: HashMap::new(),
            mode: BounceMode::Track(tid),
            range_samples: (0, 100),
            bpm: 120.0,
            sample_rate: 44_100,
        };
        let out = render_offline(&req);
        let peak = out.audio.channels[0]
            .iter()
            .map(|s| s.abs())
            .fold(0.0_f32, f32::max);
        let expected = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
        assert!((peak - expected).abs() < 1e-3, "peak {peak}");
    }
}
