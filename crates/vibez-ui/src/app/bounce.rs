//! Bounce-to-audio: render note/audio clips offline through the
//! engine and swap the result into the arrangement.

//! Split out of app.rs; inherent methods on [`super::App`].

use std::path::PathBuf;
use std::sync::Arc;

use iced::Task;

use vibez_core::id::{ClipId, TrackId};
use vibez_engine::commands::EngineCommand;

use crate::message::Message;
use crate::state::{ArrangementSelection, ProjectTrack, UiClip};

use super::*;

pub(super) struct BounceAssets {
    pub(super) clips:
        std::collections::HashMap<ClipId, Arc<vibez_core::audio_buffer::DecodedAudio>>,
    pub(super) samplers:
        std::collections::HashMap<TrackId, (Arc<vibez_core::audio_buffer::DecodedAudio>, String)>,
    pub(super) pads: std::collections::HashMap<
        (TrackId, usize),
        (Arc<vibez_core::audio_buffer::DecodedAudio>, String),
    >,
}

impl App {
    pub(super) fn collect_bounce_assets(&self) -> BounceAssets {
        let mut clips = std::collections::HashMap::new();
        let mut samplers = std::collections::HashMap::new();
        let mut pads = std::collections::HashMap::new();
        for track in &self.state.project_tracks.tracks {
            if let Some(content) = self.state.arrange_content(track.id) {
                for clip in &content.clips {
                    clips.insert(clip.id, Arc::clone(&clip.audio));
                }
            }
            if let Some(audio) = &track.sample_audio {
                samplers.insert(
                    track.id,
                    (
                        Arc::clone(audio),
                        track.sample_name.clone().unwrap_or_default(),
                    ),
                );
            }
            for (i, pad) in track.drum_rack_pads.iter().enumerate() {
                if let Some(audio) = &pad.audio {
                    pads.insert(
                        (track.id, i),
                        (Arc::clone(audio), pad.name.clone().unwrap_or_default()),
                    );
                }
            }
        }
        BounceAssets {
            clips,
            samplers,
            pads,
        }
    }

    pub(super) fn next_bounce_path(&self) -> PathBuf {
        let base = match &self.state.project.current_path {
            Some(project_path) => project_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join("renders"),
            None => std::env::temp_dir().join("vibez-renders"),
        };
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        base.join(format!("bounce-{stamp}.wav"))
    }

    pub(super) fn dispatch_bounce(
        &mut self,
        mode: vibez_engine::render::BounceMode,
        range_samples: (u64, u64),
        insert_position_samples: u64,
        clip_name: String,
    ) -> Task<Message> {
        if range_samples.1 <= range_samples.0 {
            self.state.status_text = "Empty range, nothing to bounce".to_string();
            return Task::none();
        }

        let assets = self.collect_bounce_assets();
        let project = self.project_from_state();
        let wav_path = self.next_bounce_path();
        let sample_rate = self.state.transport.sample_rate;
        let bpm = self.state.transport.bpm;

        let request = vibez_engine::render::BounceRequest {
            tracks: project.tracks,
            master: project.master,
            buses: project.buses,
            audio_clips: project.clips,
            note_clips: project.note_clips,
            clip_audio: assets.clips,
            sampler_audio: assets.samplers,
            drum_pad_audio: assets.pads,
            mode,
            range_samples,
            bpm,
            sample_rate,
        };

        self.state.status_text = format!("Bouncing {clip_name}...");
        Task::perform(
            bounce_async(request, wav_path, clip_name, insert_position_samples),
            Message::BounceComplete,
        )
    }

    pub(super) fn finalize_bounce(&mut self, outcome: crate::message::BounceOutcome) {
        let track_num = self.next_unique_track_number("Bounce");
        Arc::make_mut(&mut self.state.project_tracks).next_track_number = track_num + 1;
        let color_index = (track_num.wrapping_sub(1) % 8) as u8;
        let track_id = TrackId::new();
        let track_name = format!("Bounce {track_num}");

        self.send_command(EngineCommand::AddTrack(track_id, track_name.clone()));
        Arc::make_mut(&mut self.state.project_tracks)
            .tracks
            .push(ProjectTrack::new(track_id, track_name, color_index));

        let clip_id = ClipId::new();
        let duration = outcome.audio.num_frames() as u64;
        self.send_command(EngineCommand::AddClip {
            track_id,
            clip_id,
            audio: Arc::clone(&outcome.audio),
            position: outcome.insert_position_samples,
            source_offset: 0,
            duration,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        });

        if self.state.find_track(track_id).is_some() {
            self.state.arrange_content_mut(track_id).clips.push(UiClip {
                id: clip_id,
                name: outcome.clip_name.clone(),
                audio: Arc::clone(&outcome.audio),
                source: Some(outcome.source.clone()),
                position: outcome.insert_position_samples,
                source_offset: 0,
                duration,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
                original_bpm: None,
                warped: false,
                warped_to_bpm: None,
                original_audio: None,
            });
        }

        self.state.arrangement.selected_track = Some(track_id);
        self.state.arrangement.selected_clips.clear();
        self.state
            .arrangement
            .selected_clips
            .insert(ArrangementSelection::AudioClip { track_id, clip_id });
        self.mark_project_dirty();

        let warnings_note = if outcome.warnings.is_empty() {
            String::new()
        } else {
            format!(" ({} warning(s))", outcome.warnings.len())
        };
        self.state.status_text = format!(
            "Bounced '{}' to {}{}",
            outcome.clip_name,
            outcome.path.display(),
            warnings_note
        );
    }

    pub(super) fn handle_bounce_clip_to_audio(
        &mut self,
        track_id: TrackId,
        clip_id: ClipId,
        is_note_clip: bool,
    ) -> Task<Message> {
        self.state.view.context_menu = None;
        let (range, insert_pos, name) = if is_note_clip {
            let spb = self.state.transport.sample_rate as f64 * 60.0 / self.state.transport.bpm;
            let nc = self
                .state
                .arrange_content(track_id)
                .and_then(|content| content.note_clips.iter().find(|c| c.id == clip_id));
            match nc {
                Some(nc) => {
                    let start = (nc.position_beats * spb) as u64;
                    let end = ((nc.position_beats + nc.duration_beats) * spb) as u64;
                    (Some((start, end)), start, nc.name.clone())
                }
                None => (None, 0, String::new()),
            }
        } else {
            let ac = self
                .state
                .arrange_content(track_id)
                .and_then(|content| content.clips.iter().find(|c| c.id == clip_id));
            match ac {
                Some(ac) => (
                    Some((ac.position, ac.position + ac.duration)),
                    ac.position,
                    ac.name.clone(),
                ),
                None => (None, 0, String::new()),
            }
        };
        let Some(range) = range else {
            self.state.status_text = "Clip not found".to_string();
            return Task::none();
        };
        self.dispatch_bounce(
            vibez_engine::render::BounceMode::Clip {
                track_id,
                clip_id,
                is_note_clip,
            },
            range,
            insert_pos,
            name,
        )
    }
}
