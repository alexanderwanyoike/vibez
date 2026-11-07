//! Join selected Audio Clips into one Clip holding their consolidated audio.

use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::{ClipId, TrackId};
use vibez_engine::commands::EngineCommand;

use super::EngineHandle;
use crate::state::{ArrangementSelection, TimelineEditorState, UiClip};

/// The audible result of joining Audio Clips, laid out from the first Clip.
#[derive(Debug, Clone)]
pub struct JoinedAudio {
    pub position: u64,
    pub duration: u64,
    pub loop_enabled: bool,
    pub audio: Arc<DecodedAudio>,
}

/// Consolidates `clips` (sorted by position) into one buffer, including
/// gain, fades and source wrapping for Clips whose visible duration exceeds
/// their loop.
pub fn render_joined_audio(clips: &[UiClip]) -> JoinedAudio {
    let position = clips[0].position;
    let end = clips
        .iter()
        .map(|clip| clip.position.saturating_add(clip.duration))
        .max()
        .unwrap_or(position);
    let duration = end - position;
    let channels = clips[0].audio.num_channels();
    let mut joined: Vec<Vec<f32>> = (0..channels)
        .map(|_| vec![0.0f32; duration as usize])
        .collect();
    for clip in clips {
        let offset = (clip.position - position) as usize;
        let channel_count = channels.min(clip.audio.num_channels());
        for (channel, dst) in joined.iter_mut().enumerate().take(channel_count) {
            for local in 0..clip.duration {
                let Some(frame) = dst.get_mut(offset + local as usize) else {
                    break;
                };
                let source_frame = clip.source_frame_at(local) as usize;
                if let Some(sample) = clip.audio.channels[channel].get(source_frame) {
                    *frame +=
                        *sample * clip.gain_db.linear() * clip.fades.gain_at(local, clip.duration);
                }
            }
        }
    }
    JoinedAudio {
        position,
        duration,
        loop_enabled: clips.iter().any(|clip| clip.loop_enabled),
        audio: Arc::new(DecodedAudio {
            channels: joined,
            sample_rate: clips[0].audio.sample_rate,
        }),
    }
}

impl TimelineEditorState {
    pub(super) fn join_audio_clips(
        &mut self,
        track_id: TrackId,
        selections: &[ArrangementSelection],
        engine: &mut impl EngineHandle,
    ) -> Option<String> {
        let mut clips: Vec<UiClip> = Vec::new();
        if let Some(track) = self.find_content(track_id) {
            for selection in selections {
                if let ArrangementSelection::AudioClip { clip_id, .. } = selection {
                    if let Some(clip) = track.clips.iter().find(|c| c.id == *clip_id) {
                        clips.push(clip.clone());
                    }
                }
            }
        }
        if clips.len() < 2 {
            return None;
        }
        clips.sort_by_key(|clip| clip.position);
        let joined = render_joined_audio(&clips);
        let clip_ids: Vec<ClipId> = clips.iter().map(|clip| clip.id).collect();
        self.replace_with_joined_clip(engine, track_id, &clip_ids, joined);
        Some("Joined audio clips".to_string())
    }

    fn replace_with_joined_clip(
        &mut self,
        engine: &mut impl EngineHandle,
        track_id: TrackId,
        originals: &[ClipId],
        joined: JoinedAudio,
    ) {
        for clip_id in originals {
            self.unlink_crossfades_for_clip(engine, track_id, *clip_id);
            engine.send(EngineCommand::RemoveClip(track_id, *clip_id));
            if let Some(track) = self.find_content_mut(track_id) {
                track.clips.retain(|c| c.id != *clip_id);
            }
        }

        let new_id = ClipId::new();
        engine.send(EngineCommand::AddClip {
            insertion: vibez_engine::clip_insertion::PreparedClipInsertion::new(),
            track_id,
            clip_id: new_id,
            audio: Arc::clone(&joined.audio),
            position: joined.position,
            source_offset: 0,
            start_marker: 0,
            duration: joined.duration,
            loop_enabled: joined.loop_enabled,
            loop_start: 0,
            loop_end: joined.duration,
            linear_gain: 1.0,
            fades: Default::default(),
            playback_direction: Default::default(),
            warp_markers: Default::default(),
        });
        if let Some(track) = self.find_content_mut(track_id) {
            track.clips.push(UiClip {
                id: new_id,
                name: "Joined".to_string(),
                audio: joined.audio,
                source: None,
                position: joined.position,
                source_offset: 0,
                start_marker: 0,
                duration: joined.duration,
                loop_enabled: joined.loop_enabled,
                loop_start: 0,
                loop_end: joined.duration,
                gain_db: Default::default(),
                fades: Default::default(),
                playback_direction: Default::default(),
                transient_markers: Default::default(),
                warp_markers: Default::default(),
                transpose: Default::default(),
                original_bpm: None,
                warped: false,
                warped_to_bpm: None,
                original_audio: None,
            });
        }

        self.selected_clips.clear();
        self.selected_clips.insert(ArrangementSelection::AudioClip {
            track_id,
            clip_id: new_id,
        });
    }
}
