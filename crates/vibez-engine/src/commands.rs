use std::sync::Arc;
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::effect::EffectType;
use vibez_core::id::{ClipId, EffectId, TrackId};
use vibez_core::midi::{InstrumentKind, MidiNote};
use vibez_core::track::DrumPadState;
use vibez_dsp::effect::AudioEffect;
use vibez_instruments::Instrument;

/// Commands sent from the UI thread to the audio engine (via rtrb).
///
/// These are pushed into an `rtrb::Producer<EngineCommand>` on the UI side
/// and drained from an `rtrb::Consumer<EngineCommand>` inside the real-time
/// audio callback.  Every variant must be safe to construct on the UI thread
/// and safe to drop on the audio thread without blocking.
pub enum EngineCommand {
    /// Start playback from the current transport position.
    Play,
    /// Stop playback (transport position is preserved).
    Stop,
    /// Seek the transport to an absolute sample position.
    Seek(u64),
    /// Change the project tempo.
    SetBpm(f64),
    /// Load decoded audio into the engine for playback (legacy single-file).
    LoadAudio(Arc<DecodedAudio>),
    /// Remove any loaded audio from the engine (legacy single-file).
    UnloadAudio,

    // -- Multi-track commands --
    /// Add a new track with the given ID and name.
    AddTrack(TrackId, String),
    /// Remove a track by ID.
    RemoveTrack(TrackId),
    /// Reorder tracks to match the given ID order.
    ReorderTracks(Vec<TrackId>),
    /// Add a clip to a track.
    AddClip {
        track_id: TrackId,
        clip_id: ClipId,
        audio: Arc<DecodedAudio>,
        position: u64,
        source_offset: u64,
        duration: u64,
        loop_enabled: bool,
        loop_start: u64,
        loop_end: u64,
    },
    /// Remove a clip from a track.
    RemoveClip(TrackId, ClipId),
    /// Swap the audio buffer backing an existing clip in place. Used
    /// by the warp / quantize pipelines to install a stretched buffer
    /// without round-tripping through RemoveClip + AddClip (which
    /// would break playback continuity and lose selection state).
    /// The caller is responsible for scaling `duration`,
    /// `source_offset`, and loop bounds by the stretch ratio.
    ReplaceClipAudio {
        track_id: TrackId,
        clip_id: ClipId,
        audio: Arc<DecodedAudio>,
        duration: u64,
        source_offset: u64,
        loop_start: u64,
        loop_end: u64,
    },
    /// Move a clip to a new position on the timeline.
    MoveClip {
        track_id: TrackId,
        clip_id: ClipId,
        new_position: u64,
    },
    /// Set the gain for a track.
    SetTrackGain(TrackId, f32),
    /// Set the pan for a track (0.0 = left, 0.5 = center, 1.0 = right).
    SetTrackPan(TrackId, f32),
    /// Set the mute state for a track.
    SetTrackMute(TrackId, bool),
    /// Set the solo state for a track.
    SetTrackSolo(TrackId, bool),

    // -- Infrastructure --
    SetSampleRate(u32),

    // -- Effects --
    AddEffect {
        track_id: TrackId,
        effect_id: EffectId,
        effect_type: EffectType,
        position: Option<usize>,
    },
    RemoveEffect(TrackId, EffectId),
    SetEffectParam {
        track_id: TrackId,
        effect_id: EffectId,
        param_index: usize,
        value: f32,
    },
    SetEffectBypass {
        track_id: TrackId,
        effect_id: EffectId,
        bypass: bool,
    },
    MoveEffect {
        track_id: TrackId,
        effect_id: EffectId,
        new_index: usize,
    },

    // -- Instrument tracks --
    AddInstrumentTrack(TrackId, String, InstrumentKind),
    /// Add a bare MIDI track (no synth attached).
    AddMidiTrack(TrackId, String),
    /// Attach an instrument to a track.
    SetTrackInstrument(TrackId, InstrumentKind),
    /// Remove the instrument from a track.
    RemoveTrackInstrument(TrackId),
    /// Set note clip duration (for halve/double).
    SetNoteClipDuration {
        track_id: TrackId,
        clip_id: ClipId,
        duration_beats: f64,
    },
    AddNoteClip {
        track_id: TrackId,
        clip_id: ClipId,
        position_beats: f64,
        duration_beats: f64,
        loop_enabled: bool,
        loop_start_beats: f64,
        loop_end_beats: f64,
    },
    RemoveNoteClip(TrackId, ClipId),
    /// Move a note clip to a new beat position on the timeline.
    MoveNoteClip {
        track_id: TrackId,
        clip_id: ClipId,
        new_position_beats: f64,
    },
    AddNote {
        track_id: TrackId,
        clip_id: ClipId,
        note: MidiNote,
    },
    RemoveNote {
        track_id: TrackId,
        clip_id: ClipId,
        note_index: usize,
    },
    EditNote {
        track_id: TrackId,
        clip_id: ClipId,
        note_index: usize,
        note: MidiNote,
    },
    SetInstrumentParam {
        track_id: TrackId,
        param_index: usize,
        value: f32,
    },
    LoadSamplerSample {
        track_id: TrackId,
        sample: Arc<DecodedAudio>,
        sample_name: String,
    },
    LoadDrumRackPadSample {
        track_id: TrackId,
        pad_index: usize,
        sample: Arc<DecodedAudio>,
        sample_name: String,
    },
    ClearDrumRackPad {
        track_id: TrackId,
        pad_index: usize,
    },
    SetDrumRackPadState {
        track_id: TrackId,
        pad_index: usize,
        state: DrumPadState,
    },

    // -- Arrangement loop --
    SetArrangementLoop(bool),
    SetArrangementLoopRegion {
        start: u64,
        end: u64,
    },

    // -- Clip looping --
    SetClipLoop {
        track_id: TrackId,
        clip_id: ClipId,
        enabled: bool,
        loop_start: u64,
        loop_end: u64,
    },
    SetNoteClipLoop {
        track_id: TrackId,
        clip_id: ClipId,
        enabled: bool,
        loop_start_beats: f64,
        loop_end_beats: f64,
    },

    // -- Preview (sample auditioning) --
    /// Start previewing a decoded audio buffer on the hidden preview
    /// channel. Bypasses transport, mute, and solo; one-shot; the
    /// previous preview is cut if still playing.
    StartPreview(Arc<DecodedAudio>),
    /// Stop any in-progress preview.
    StopPreview,

    // -- External MIDI input --
    /// Route a live note-on from an external MIDI source (hardware
    /// keyboard, Push, virtual cable) to the instrument on the named
    /// track. Routed outside the clip pipeline so it works regardless
    /// of transport state.
    ExternalNoteOn {
        track_id: TrackId,
        pitch: u8,
        velocity: u8,
    },
    /// Route a live note-off from an external MIDI source.
    ExternalNoteOff {
        track_id: TrackId,
        pitch: u8,
    },

    // -- External plugins --
    /// Add a pre-loaded external plugin effect to a track.
    AddPluginEffect {
        track_id: TrackId,
        effect_id: EffectId,
        effect: Box<dyn AudioEffect>,
        position: Option<usize>,
    },
    /// Set a pre-loaded external plugin instrument on a track.
    /// Audition a single note on a track's instrument (piano-roll
    /// key press / drum pad click). Works while the transport is
    /// stopped thanks to idle instrument rendering.
    AuditionNote {
        track_id: TrackId,
        pitch: u8,
        velocity: u8,
        on: bool,
    },
    SetPluginInstrument {
        track_id: TrackId,
        instrument: Box<dyn Instrument>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_variants_are_constructible() {
        let _play = EngineCommand::Play;
        let _stop = EngineCommand::Stop;
        let _seek = EngineCommand::Seek(44_100);
        let _bpm = EngineCommand::SetBpm(140.0);

        let audio = Arc::new(DecodedAudio {
            channels: vec![vec![0.0; 100], vec![0.0; 100]],
            sample_rate: 44_100,
        });
        let _load = EngineCommand::LoadAudio(audio);
        let _unload = EngineCommand::UnloadAudio;
    }

    #[test]
    fn multitrack_command_variants() {
        let tid = TrackId::new();
        let cid = ClipId::new();
        let audio = Arc::new(DecodedAudio {
            channels: vec![vec![0.0; 100]],
            sample_rate: 44_100,
        });

        let _add_track = EngineCommand::AddTrack(tid, "Track 1".into());
        let _remove_track = EngineCommand::RemoveTrack(tid);
        let _add_clip = EngineCommand::AddClip {
            track_id: tid,
            clip_id: cid,
            audio,
            position: 0,
            source_offset: 0,
            duration: 100,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        };
        let _remove_clip = EngineCommand::RemoveClip(tid, cid);
        let _move_clip = EngineCommand::MoveClip {
            track_id: tid,
            clip_id: cid,
            new_position: 500,
        };
        let _gain = EngineCommand::SetTrackGain(tid, 0.8);
        let _pan = EngineCommand::SetTrackPan(tid, 0.3);
        let _mute = EngineCommand::SetTrackMute(tid, true);
        let _solo = EngineCommand::SetTrackSolo(tid, true);
        let _reorder = EngineCommand::ReorderTracks(vec![tid]);
    }

    #[test]
    fn command_can_be_sent_through_rtrb() {
        let (mut producer, mut consumer) = rtrb::RingBuffer::<EngineCommand>::new(16);

        producer.push(EngineCommand::Play).unwrap();
        producer.push(EngineCommand::Seek(1000)).unwrap();
        producer.push(EngineCommand::SetBpm(90.0)).unwrap();
        producer.push(EngineCommand::Stop).unwrap();

        let cmd = consumer.pop().unwrap();
        assert!(matches!(cmd, EngineCommand::Play));

        let cmd = consumer.pop().unwrap();
        assert!(matches!(cmd, EngineCommand::Seek(1000)));

        let cmd = consumer.pop().unwrap();
        match cmd {
            EngineCommand::SetBpm(bpm) => assert!((bpm - 90.0).abs() < f64::EPSILON),
            _ => panic!("expected SetBpm"),
        }

        let cmd = consumer.pop().unwrap();
        assert!(matches!(cmd, EngineCommand::Stop));
    }

    #[test]
    fn reorder_tracks_command_through_rtrb() {
        let (mut producer, mut consumer) = rtrb::RingBuffer::<EngineCommand>::new(16);
        let tid1 = TrackId::new();
        let tid2 = TrackId::new();

        producer
            .push(EngineCommand::ReorderTracks(vec![tid2, tid1]))
            .unwrap();
        let cmd = consumer.pop().unwrap();
        match cmd {
            EngineCommand::ReorderTracks(order) => {
                assert_eq!(order.len(), 2);
                assert_eq!(order[0], tid2);
                assert_eq!(order[1], tid1);
            }
            _ => panic!("expected ReorderTracks"),
        }
    }

    #[test]
    fn load_audio_shares_arc() {
        let audio = Arc::new(DecodedAudio {
            channels: vec![vec![1.0, 2.0]],
            sample_rate: 48_000,
        });
        let cmd = EngineCommand::LoadAudio(Arc::clone(&audio));

        // The Arc should have 2 strong references now.
        assert_eq!(Arc::strong_count(&audio), 2);

        match cmd {
            EngineCommand::LoadAudio(a) => {
                assert_eq!(a.num_frames(), 2);
                assert_eq!(a.sample_rate, 48_000);
            }
            _ => panic!("expected LoadAudio"),
        }
    }
}
