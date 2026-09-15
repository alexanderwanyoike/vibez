//! Browser imports land in independent Clip slots without touching Arrange.

use std::sync::Arc;

use iced::Task;
use vibez_core::id::{ClipId, TrackId};
use vibez_project::{PerformLayout, TimelineLocation};

use crate::domains::perform::LauncherClip;
use crate::message::{Message, PreparedBrowserImport};
use crate::state::{ArrangementTimeline, AuditionMode, UiClip};

use super::App;

impl App {
    pub(super) fn add_audio_launcher_clip(
        &mut self,
        track_id: TrackId,
        row: u32,
        payload: PreparedBrowserImport,
    ) -> Task<Message> {
        if self.state.perform.layout != PerformLayout::Clips
            || self
                .state
                .find_track(track_id)
                .is_none_or(|track| track.kind.is_midi())
            || self.state.perform.clips.at(track_id, row).is_some()
        {
            self.state.status_text = "Clip slot is no longer available".into();
            return Task::none();
        }
        let PreparedBrowserImport {
            treatment,
            audio,
            original_audio,
            name,
            source,
        } = payload;
        let id = ClipId::new();
        let duration = audio.num_frames() as u64;
        let warped = treatment.mode == AuditionMode::Warp;
        let clip = UiClip {
            id,
            name: name.clone(),
            audio,
            original_audio,
            source: Some(source),
            position: 0,
            source_offset: 0,
            start_marker: 0,
            duration,
            loop_enabled: true,
            loop_start: 0,
            loop_end: duration,
            gain_db: Default::default(),
            fades: Default::default(),
            playback_direction: Default::default(),
            transient_markers: Default::default(),
            warp_markers: Default::default(),
            transpose: Default::default(),
            original_bpm: treatment.source_bpm,
            warped,
            warped_to_bpm: warped.then_some(self.state.transport.bpm),
        };
        let mut timeline = ArrangementTimeline::default();
        timeline.ensure(track_id).clips.push(clip);
        Arc::make_mut(&mut self.state.perform.clips)
            .clips
            .push(LauncherClip {
                id,
                track_id,
                row,
                timeline: Arc::new(timeline),
            });
        self.state.perform.select_launcher_clip(id);
        self.state.view.detail_panel_tab = crate::state::DetailPanelTab::Clip;
        self.state.arrangement.selected_track = Some(track_id);
        self.state.status_text = format!("Added {name} to Clip row {}", row + 1);
        self.mark_project_dirty();
        self.schedule_auto_detect_clip_transients(TimelineLocation::LauncherClip(id), track_id, id)
    }
}

impl App {
    pub(super) fn launch_clips(&mut self, request: crate::domains::perform::ClipLaunchRequest) {
        use crate::domains::perform::ClipLaunchRequest;
        use vibez_engine::{commands::EngineCommand, playback_source::PreparedClipPlayback};
        if self.state.perform.layout != PerformLayout::Clips {
            return;
        }
        let targets: Vec<_> = match request {
            ClipLaunchRequest::Clip(id) => self
                .state
                .perform
                .clips
                .by_id(id)
                .map(|clip| vec![(clip.track_id, Some(clip.clone()))])
                .unwrap_or_default(),
            ClipLaunchRequest::Stop(track_id) => vec![(track_id, None)],
            ClipLaunchRequest::Row(row) => self
                .state
                .project_tracks
                .tracks
                .iter()
                .map(|track| {
                    (
                        track.id,
                        self.state.perform.clips.at(track.id, row).cloned(),
                    )
                })
                .collect(),
            ClipLaunchRequest::StopAll => self
                .state
                .project_tracks
                .tracks
                .iter()
                .map(|track| (track.id, None))
                .collect(),
        };
        if !self.state.perform.clip_editor.running && targets.iter().all(|(_, clip)| clip.is_none())
        {
            return;
        }
        if targets
            .iter()
            .filter_map(|(_, clip)| clip.as_ref())
            .any(|clip| {
                clip.timeline.get(clip.track_id).is_some_and(|content| {
                    content
                        .clips
                        .iter()
                        .any(|audio| audio.audio.num_frames() == 0)
                })
            })
        {
            self.state.status_text = "Clip media is not ready".into();
            return;
        }
        let samples_per_beat =
            self.state.transport.sample_rate as f64 * 60.0 / self.state.transport.bpm;
        let mut prepared = Vec::new();
        for (track_id, clip) in targets {
            self.state.perform.clip_editor.next_request += 1;
            let request_id = self.state.perform.clip_editor.next_request;
            if let Some(clip) = clip {
                prepared.push(clip.prepare(request_id, samples_per_beat));
                self.state
                    .perform
                    .clip_editor
                    .pending
                    .insert(request_id, clip);
            } else {
                prepared.push(Box::new(PreparedClipPlayback {
                    track_id,
                    clip_id: None,
                    request_id,
                    length_samples: 1,
                    looping: false,
                    source: Box::default(),
                }));
            }
        }
        if !prepared.is_empty() {
            self.state.perform.clip_editor.running = true;
            self.send_command(EngineCommand::QueueClips {
                clips: prepared,
                quantization: vibez_core::perform::MusicalBoundary::OneBar,
            });
        }
    }
}
