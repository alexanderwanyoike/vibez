use std::sync::Arc;

use rtrb::Producer;
use vibez_core::audio_buffer::DecodedAudio;

use crate::events::EngineEvent;
use crate::transport::Transport;

/// How many replaced voices can fade out simultaneously. With tiny
/// audio buffers a retrigger can arrive before the previous ~5ms fade
/// finishes; a couple of fixed slots absorb the overlap without
/// clicking or allocating on the audio thread.
const AUDITION_OUTGOING_VOICES: usize = 3;
const AUDITION_FADE_MS: usize = 5;

/// Dedicated Browser signal path mixed after project master processing.
pub(super) struct AuditionBus {
    pub(super) active: Option<AuditionVoice>,
    outgoing: [Option<AuditionVoice>; AUDITION_OUTGOING_VOICES],
    pub(super) queued: Option<QueuedAudition>,
    pub(super) gain: f32,
}

pub(super) struct AuditionVoice {
    audio: Arc<DecodedAudio>,
    position: u64,
    fade_in_frames: usize,
    fade_out_remaining: Option<usize>,
    looped: bool,
    synchronized: bool,
}

pub(super) struct QueuedAudition {
    audio: Arc<DecodedAudio>,
    frames_until_start: u64,
    looped: bool,
    synchronized: bool,
}

impl AuditionBus {
    pub(super) fn new() -> Self {
        Self {
            active: None,
            outgoing: std::array::from_fn(|_| None),
            queued: None,
            gain: 1.0,
        }
    }

    pub(super) fn start(
        &mut self,
        audio: Arc<DecodedAudio>,
        fade_frames: usize,
        looped: bool,
        synchronized: bool,
    ) {
        self.queued = None;
        self.stop_active(fade_frames);
        self.active = Some(AuditionVoice {
            audio,
            position: 0,
            fade_in_frames: fade_frames,
            fade_out_remaining: None,
            looped,
            synchronized,
        });
    }

    pub(super) fn queue(
        &mut self,
        audio: Arc<DecodedAudio>,
        frames_until_start: u64,
        fade_frames: usize,
        looped: bool,
        synchronized: bool,
    ) {
        self.stop_active(fade_frames);
        self.queued = Some(QueuedAudition {
            audio,
            frames_until_start,
            looped,
            synchronized,
        });
    }

    /// A synchronized preview may begin while the Project is stopped. When
    /// playback later starts, restart it on the current bar boundary or queue
    /// it to the next one so it cannot remain free-running beside the Project.
    pub(super) fn resync_on_transport_start(
        &mut self,
        position: u64,
        bpm: f64,
        sample_rate: u32,
        fade_frames: usize,
    ) -> Option<bool> {
        let active = self.active.as_ref().filter(|voice| voice.synchronized)?;
        let audio = Arc::clone(&active.audio);
        let looped = active.looped;
        let frames_until_start = frames_until_audition_boundary(position, bpm, sample_rate, 4);
        if frames_until_start == 0 {
            self.start(audio, fade_frames, looped, true);
            Some(false)
        } else {
            self.queue(audio, frames_until_start, fade_frames, looped, true);
            Some(true)
        }
    }

    pub(super) fn stop(&mut self, fade_frames: usize) {
        self.queued = None;
        self.stop_active(fade_frames);
    }

    fn stop_active(&mut self, fade_frames: usize) {
        if let Some(mut active) = self.active.take() {
            if active.position > 0 {
                active.fade_out_remaining = Some(fade_frames);
                // Prefer a free slot; when a rapid retrigger burst has
                // every slot fading, replace the voice closest to done
                // (the smallest possible click).
                let slot = self
                    .outgoing
                    .iter_mut()
                    .min_by_key(|slot| {
                        slot.as_ref()
                            .map_or(0, |voice| voice.fade_out_remaining.unwrap_or(0))
                    })
                    .expect("outgoing slots are non-empty");
                *slot = Some(active);
            }
        }
    }

    pub(super) fn has_outgoing(&self) -> bool {
        self.outgoing.iter().any(Option::is_some)
    }

    pub(super) fn process(
        &mut self,
        output: &mut [f32],
        frames: usize,
        channels: usize,
        transport: &Transport,
        sample_rate: u32,
        event_tx: &mut Producer<EngineEvent>,
    ) {
        let mut had_voice = self.active.is_some() || self.has_outgoing();
        let mut start_offset = 0;
        let queued_ready = self.queued.as_ref().is_some_and(|queued| {
            if !transport.is_playing() {
                return true;
            }
            queued.frames_until_start < frames as u64
        });
        if queued_ready {
            let queued = self.queued.take().expect("queued audition exists");
            if transport.is_playing() {
                start_offset = queued.frames_until_start.min(frames as u64) as usize;
            }
            self.start(
                queued.audio,
                audition_fade_frames(sample_rate),
                queued.looped,
                queued.synchronized,
            );
            had_voice = true;
            let _ = event_tx.push(EngineEvent::AuditionStarted);
        } else if transport.is_playing() {
            if let Some(queued) = self.queued.as_mut() {
                queued.frames_until_start = queued.frames_until_start.saturating_sub(frames as u64);
            }
        }
        let gain = self.gain;
        for slot in self.outgoing.iter_mut() {
            if slot.as_mut().is_some_and(|voice| {
                render_audition_voice(voice, output, frames, channels, gain, 0)
            }) {
                *slot = None;
            }
        }
        if self.active.as_mut().is_some_and(|voice| {
            render_audition_voice(voice, output, frames, channels, gain, start_offset)
        }) {
            self.active = None;
        }
        if had_voice && self.active.is_none() && !self.has_outgoing() && self.queued.is_none() {
            let _ = event_tx.push(EngineEvent::AuditionStopped);
        }
    }
}

pub(super) fn audition_fade_frames(sample_rate: u32) -> usize {
    ((sample_rate as usize * AUDITION_FADE_MS) / 1_000).max(1)
}

pub(super) fn next_audition_boundary(position: u64, bpm: f64, sample_rate: u32, beats: u64) -> u64 {
    if bpm <= 0.0 || sample_rate == 0 {
        return position;
    }
    let boundary_frames = sample_rate as f64 * 60.0 / bpm * beats.max(1) as f64;
    let boundary_index = (position as f64 / boundary_frames).floor() + 1.0;
    (boundary_index * boundary_frames).round() as u64
}

fn frames_until_audition_boundary(position: u64, bpm: f64, sample_rate: u32, beats: u64) -> u64 {
    if bpm <= 0.0 || sample_rate == 0 {
        return 0;
    }
    let boundary_frames = sample_rate as f64 * 60.0 / bpm * beats.max(1) as f64;
    let nearest = ((position as f64 / boundary_frames).round() * boundary_frames).round() as u64;
    if nearest == position {
        0
    } else {
        next_audition_boundary(position, bpm, sample_rate, beats).saturating_sub(position)
    }
}

/// Mix one audition voice. Returns true once its source or fade is done.
fn render_audition_voice(
    voice: &mut AuditionVoice,
    output: &mut [f32],
    frames: usize,
    channels: usize,
    bus_gain: f32,
    output_frame_offset: usize,
) -> bool {
    let audio_channels = voice.audio.num_channels();
    let audio_frames = voice.audio.num_frames();
    if audio_channels == 0 || audio_frames == 0 || channels == 0 {
        return true;
    }

    let render_frames = frames.saturating_sub(output_frame_offset);
    for frame in 0..render_frames {
        let mut source = voice.position as usize;
        if source >= audio_frames {
            if !voice.looped {
                return true;
            }
            // A loop voice must consume exactly one source-length of output
            // per wrap or it will drift ahead of the Project transport. Fade
            // the boundary in place instead of overlap-skipping head frames.
            source = 0;
            voice.position = 0;
        }
        let attack = if source < voice.fade_in_frames {
            (source + 1) as f32 / voice.fade_in_frames as f32
        } else {
            1.0
        };
        let remaining_source = audio_frames.saturating_sub(source + 1);
        let natural_release =
            (remaining_source as f32 / voice.fade_in_frames as f32).clamp(0.0, 1.0);
        let commanded_release = match voice.fade_out_remaining {
            Some(remaining) => {
                let envelope = remaining.saturating_sub(1) as f32 / voice.fade_in_frames as f32;
                voice.fade_out_remaining = Some(remaining.saturating_sub(1));
                envelope
            }
            None => 1.0,
        };
        let envelope = attack.min(natural_release) * commanded_release * bus_gain;
        for ch in 0..channels {
            let source_channel = ch.min(audio_channels - 1);
            let sample = voice.audio.sample(source_channel, source);
            output[(output_frame_offset + frame) * channels + ch] += sample * envelope;
        }
        voice.position = voice.position.saturating_add(1);
        if voice.fade_out_remaining == Some(0) {
            return true;
        }
    }

    !voice.looped && voice.position as usize >= audio_frames
}
