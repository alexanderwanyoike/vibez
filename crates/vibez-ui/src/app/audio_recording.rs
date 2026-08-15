//! Arrange hardware-input recording lifecycle and Project Media commit.

use std::path::Path;
use std::sync::Arc;

use iced::Task;
use vibez_audio_io::audio_input::{AudioInputEvent, AudioInputStream};
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::TrackKind;
use vibez_core::track::{AudioInputRoute, InputMonitoring};
use vibez_engine::commands::EngineCommand;

use crate::domains::audio_recording::AudioRecordingPhase;
use crate::domains::transport::TransportMsg;
use crate::message::{AudioRecordingOutcome, Message};
use crate::state::{ArrangementSelection, UiClip, Workspace};

use super::*;

impl App {
    pub(super) fn toggle_audio_track_arm(&mut self, track_id: TrackId) -> Task<Message> {
        if self.state.audio_recording.is_busy() {
            self.state.status_text = "Stop the current Audio recording before changing Arm".into();
            return Task::none();
        }
        let Some(track) = self.state.find_track(track_id) else {
            return Task::none();
        };
        if track.kind != TrackKind::Audio {
            self.state.status_text = "Audio Input can only arm an Audio Project Track".into();
            return Task::none();
        }
        let name = track.name.clone();
        if self.state.audio_recording.armed_track == Some(track_id) {
            self.state.audio_recording.disarm();
            self.state.audio_recording.monitor_track = self.persisted_monitor_on_track();
            if let Err(error) = self.sync_audio_input_runtime() {
                self.state.status_text = format!("{name} disarmed · Audio Input cleanup: {error}");
                return Task::none();
            }
            self.state.status_text = format!("{name} disarmed");
        } else {
            let previous_monitor = self.state.audio_recording.monitor_track;
            self.state.audio_recording.arm(track_id);
            self.state.audio_recording.monitor_track = Some(track_id);
            if let Err(error) = self.sync_audio_input_runtime() {
                self.state.audio_recording.disarm();
                self.state.audio_recording.monitor_track = previous_monitor;
                self.input_bridge.set_target(None, false);
                self.state.status_text = format!("Could not arm {name} — {error}");
            } else {
                self.state.status_text = format!("{name} armed for Audio Input");
            }
        }
        Task::none()
    }

    pub(super) fn set_audio_track_input_route(
        &mut self,
        track_id: TrackId,
        route: AudioInputRoute,
    ) -> Task<Message> {
        if self.state.audio_recording.is_busy() {
            self.state.status_text = "Stop recording before changing the Audio Input route".into();
            return Task::none();
        }
        if !self.input_route_is_available(route) {
            self.state.status_text = "That Audio Input channel is unavailable".into();
            return Task::none();
        }
        let unchanged = self
            .state
            .find_track(track_id)
            .is_none_or(|track| track.kind != TrackKind::Audio || track.audio_input_route == route);
        if unchanged {
            return Task::none();
        }
        self.push_undo_snapshot(None);
        if let Some(track) = Arc::make_mut(&mut self.state.project_tracks).find_mut(track_id) {
            track.audio_input_route = route;
        }
        self.mark_project_dirty();
        if let Err(error) = self.sync_audio_input_runtime() {
            self.state.status_text =
                format!("Audio Input route saved, but the stream could not reopen — {error}");
        }
        Task::none()
    }

    pub(super) fn set_audio_track_monitoring(
        &mut self,
        track_id: TrackId,
        monitoring: InputMonitoring,
    ) -> Task<Message> {
        if self.state.audio_recording.is_busy() {
            self.state.status_text = "Stop recording before changing input monitoring".into();
            return Task::none();
        }
        if monitoring == InputMonitoring::On
            && self
                .state
                .audio_recording
                .armed_track
                .is_some_and(|armed| armed != track_id)
        {
            self.state.status_text =
                "Disarm the current Audio Track before monitoring a different input target".into();
            return Task::none();
        }
        let unchanged = self.state.find_track(track_id).is_none_or(|track| {
            track.kind != TrackKind::Audio || track.input_monitoring == monitoring
        });
        if unchanged {
            return Task::none();
        }
        self.push_undo_snapshot(None);
        let tracks = Arc::make_mut(&mut self.state.project_tracks);
        if monitoring == InputMonitoring::On {
            for track in &mut tracks.tracks {
                if track.id != track_id && track.input_monitoring == InputMonitoring::On {
                    track.input_monitoring = InputMonitoring::Off;
                }
            }
        }
        if let Some(track) = tracks.find_mut(track_id) {
            track.input_monitoring = monitoring;
        }
        self.state.audio_recording.monitor_track = match monitoring {
            InputMonitoring::On => Some(track_id),
            _ if self.state.audio_recording.monitor_track == Some(track_id) => {
                self.state.audio_recording.armed_track
            }
            _ => self.state.audio_recording.monitor_track,
        };
        self.mark_project_dirty();
        if let Err(error) = self.sync_audio_input_runtime() {
            self.state.status_text = format!("Input monitoring unavailable — {error}");
        }
        Task::none()
    }

    pub(super) fn toggle_audio_recording(&mut self) -> Task<Message> {
        match self.state.audio_recording.phase {
            AudioRecordingPhase::Idle => self.begin_audio_recording(),
            AudioRecordingPhase::Recording => self.stop_audio_recording(),
            AudioRecordingPhase::Stopping | AudioRecordingPhase::Finalizing => {
                self.state.status_text = "Finishing the recorded take…".into();
                Task::none()
            }
        }
    }

    fn begin_audio_recording(&mut self) -> Task<Message> {
        if self.state.view.workspace != Workspace::Arrange {
            self.state.status_text = "Audio Track Recording is available in Arrange".into();
            return Task::none();
        }
        if self.state.perform.playing_section.is_some()
            || self.state.perform.queued_section.is_some()
            || self.state.perform.section_record.is_active()
            || self.state.perform.capture.is_active()
        {
            self.state.status_text = "Stop Perform playback before Audio Track Recording".into();
            return Task::none();
        }
        if self.state.transport.loop_enabled {
            self.state.status_text = "Turn Arrange Loop off before Audio Track Recording".into();
            return Task::none();
        }
        let Some(track_id) = self.state.audio_recording.armed_track else {
            self.state.status_text = "Arm an Audio Project Track before recording".into();
            return Task::none();
        };
        if self._input_stream.is_none() {
            if let Err(error) = self.sync_audio_input_runtime() {
                self.state.status_text = format!("Audio Input could not start — {error}");
                return Task::none();
            }
        }
        if !self.begin_project_transaction() {
            self.state.status_text = "Finish the current project edit before recording".into();
            return Task::none();
        }
        let start = self.state.transport.position_samples;
        if !self.state.audio_recording.begin(start) {
            self.discard_audio_recording_transaction();
            return Task::none();
        }
        self.input_bridge.begin_recording();
        self.sync_audio_input_target();
        let track_name = self
            .state
            .find_track(track_id)
            .map_or("Audio Track", |track| track.name.as_str());
        self.state.status_text = format!("Recording Audio Input into {track_name}");
        if self.state.transport.playing {
            Task::none()
        } else {
            self.update(Message::Transport(TransportMsg::Play))
        }
    }

    pub(super) fn stop_audio_recording(&mut self) -> Task<Message> {
        if !self.state.audio_recording.request_stop() {
            return Task::none();
        }
        self.input_bridge.end_recording();
        self.state.status_text = "Finishing the recorded take…".into();
        if self.state.transport.playing {
            let _ = self.update(Message::Transport(TransportMsg::Stop));
        }
        Task::none()
    }

    fn finalize_stopped_audio_recording(&mut self) -> Task<Message> {
        self.input_bridge
            .drain_recorded(&mut self.state.audio_recording.captured_frames);
        let Some((track_id, fallback_start_position, frames)) =
            self.state.audio_recording.begin_finalizing()
        else {
            return Task::none();
        };
        let start_position_samples = self
            .input_bridge
            .record_start_position()
            .unwrap_or(fallback_start_position);
        let sample_rate = self.state.transport.sample_rate;
        self.state.status_text = "Writing recorded take to Project Media…".into();
        Task::perform(
            finalize_audio_input_take(track_id, start_position_samples, sample_rate, frames),
            Message::AudioRecordingFinalized,
        )
    }

    pub(super) fn finish_audio_recording(
        &mut self,
        result: Result<AudioRecordingOutcome, String>,
    ) -> Task<Message> {
        match result {
            Ok(outcome) => {
                let clip_id = ClipId::new();
                let duration = outcome.audio.num_frames() as u64;
                self.send_command(EngineCommand::AddClip {
                    track_id: outcome.track_id,
                    clip_id,
                    audio: Arc::clone(&outcome.audio),
                    position: outcome.start_position_samples,
                    source_offset: 0,
                    duration,
                    loop_enabled: false,
                    loop_start: 0,
                    loop_end: duration,
                });
                self.state
                    .arrange_content_mut(outcome.track_id)
                    .clips
                    .push(UiClip {
                        id: clip_id,
                        name: outcome.clip_name.clone(),
                        audio: outcome.audio,
                        source: Some(outcome.source),
                        position: outcome.start_position_samples,
                        source_offset: 0,
                        duration,
                        loop_enabled: false,
                        loop_start: 0,
                        loop_end: duration,
                        original_bpm: None,
                        warped: false,
                        warped_to_bpm: None,
                        original_audio: None,
                    });
                self.state.arrangement.selected_track = Some(outcome.track_id);
                self.state.arrangement.selected_clips.clear();
                self.state
                    .arrangement
                    .selected_clips
                    .insert(ArrangementSelection::AudioClip {
                        track_id: outcome.track_id,
                        clip_id,
                    });
                self.push_undo_snapshot(None);
                self.mark_project_dirty();
                self.commit_project_transaction();
                self.state.audio_recording.finish();
                self.sync_audio_input_target();
                self.state.status_text = format!(
                    "Recorded {} · {:.2} s · one undo step",
                    outcome.clip_name,
                    duration as f64 / self.state.transport.sample_rate as f64,
                );
            }
            Err(error) => {
                self.state.audio_recording.finish();
                self.discard_audio_recording_transaction();
                self.sync_audio_input_target();
                self.state.status_text =
                    format!("Audio recording failed — {error}. No Clip was created.");
            }
        }
        Task::none()
    }

    pub(super) fn poll_audio_input(&mut self) -> Option<Task<Message>> {
        if !self.state.audio_recording.is_busy()
            && self
                .active_audio_input_target()
                .is_some_and(|id| self.state.find_track(id).is_none())
        {
            self.state.audio_recording.armed_track = None;
            self.state.audio_recording.monitor_track = None;
            let _ = self.sync_audio_input_runtime();
        }
        let mut errors = Vec::new();
        if let Some(stream) = self._input_stream.as_ref() {
            while let Some(AudioInputEvent::Error(error)) = stream.try_next_event() {
                errors.push(error);
            }
        }
        let (peak_l, peak_r) = self.input_bridge.meter();
        self.state.audio_recording.input_peak_l = peak_l;
        self.state.audio_recording.input_peak_r = peak_r;
        if self.state.audio_recording.is_capturing() {
            self.input_bridge
                .drain_recorded(&mut self.state.audio_recording.captured_frames);
            if self.input_bridge.overflowed() {
                self.input_bridge.end_recording();
                self.state.audio_recording.captured_frames.clear();
                self.state.audio_recording.finish();
                self.discard_audio_recording_transaction();
                self.state.status_text = "Audio recording stopped — the bounded input buffer overflowed. No Clip was created.".into();
                return Some(self.update(Message::Transport(TransportMsg::Stop)));
            }
        }
        if let Some(error) = errors.into_iter().next() {
            let was_capturing = self.state.audio_recording.is_capturing();
            if was_capturing {
                self.input_bridge.end_recording();
                self.state.audio_recording.captured_frames.clear();
                self.state.audio_recording.finish();
                self.discard_audio_recording_transaction();
            }
            self._input_stream = None;
            self.state.status_text = format!("Audio Input disconnected — {error}");
            return was_capturing.then(|| self.update(Message::Transport(TransportMsg::Stop)));
        }
        if self.state.audio_recording.phase == AudioRecordingPhase::Stopping
            && self.input_bridge.recording_stopped()
        {
            return Some(self.finalize_stopped_audio_recording());
        }
        None
    }

    pub(super) fn sync_audio_input_runtime(&mut self) -> Result<(), String> {
        self.sync_audio_input_target();
        let target = self.active_audio_input_target();
        if target.is_none() {
            self._input_stream = None;
            return Ok(());
        }
        let expected_name = self.state.audio_settings.preferred_input_name.as_deref();
        let must_reopen = self._input_stream.as_ref().is_none_or(|stream| {
            stream.sample_rate != self.state.transport.sample_rate
                || expected_name.is_some_and(|name| stream.device_name != name)
        });
        if must_reopen {
            self._input_stream = None;
            let stream = AudioInputStream::open(
                expected_name,
                self.state.transport.sample_rate,
                Some(self.state.audio_settings.buffer_size),
                Arc::clone(&self.input_bridge),
            )
            .map_err(|error| error.to_string())?;
            if !route_fits_channels(self.input_bridge.route(), stream.channels as u16) {
                return Err(format!(
                    "the opened Audio Input has {} channel(s), which does not include {}",
                    stream.channels,
                    self.input_bridge.route(),
                ));
            }
            self._input_stream = Some(stream);
        }
        Ok(())
    }

    fn active_audio_input_target(&self) -> Option<TrackId> {
        self.state.audio_recording.armed_track.or_else(|| {
            self.state.audio_recording.monitor_track.filter(|id| {
                self.state
                    .find_track(*id)
                    .is_some_and(|track| track.input_monitoring == InputMonitoring::On)
            })
        })
    }

    fn persisted_monitor_on_track(&self) -> Option<TrackId> {
        self.state
            .project_tracks
            .tracks
            .iter()
            .find(|track| {
                track.kind == TrackKind::Audio && track.input_monitoring == InputMonitoring::On
            })
            .map(|track| track.id)
    }

    fn sync_audio_input_target(&self) {
        let Some(track_id) = self.active_audio_input_target() else {
            self.input_bridge.set_target(None, false);
            return;
        };
        let Some(track) = self.state.find_track(track_id) else {
            self.input_bridge.set_target(None, false);
            return;
        };
        let monitoring = match track.input_monitoring {
            InputMonitoring::Off => false,
            InputMonitoring::Auto => self.state.audio_recording.armed_track == Some(track_id),
            InputMonitoring::On => true,
        };
        self.input_bridge.set_route(track.audio_input_route);
        self.input_bridge.set_target(Some(track_id), monitoring);
    }

    fn input_route_is_available(&self, route: AudioInputRoute) -> bool {
        let channels = self
            .state
            .audio_settings
            .selected_input()
            .and_then(|device| device.default_config.as_ref().map(|config| config.channels))
            .unwrap_or(0);
        route_fits_channels(route, channels)
    }

    fn discard_audio_recording_transaction(&mut self) {
        if let Some((_, dirty_before)) = self.state.project.history.abandon_transaction() {
            self.state.project.dirty = dirty_before;
        }
    }
}

fn route_fits_channels(route: AudioInputRoute, channels: u16) -> bool {
    match route {
        AudioInputRoute::Mono { channel } => channel < channels,
        AudioInputRoute::Stereo { left } => left.saturating_add(1) < channels,
    }
}

async fn finalize_audio_input_take(
    track_id: TrackId,
    start_position_samples: u64,
    sample_rate: u32,
    frames: Vec<[f32; 2]>,
) -> Result<AudioRecordingOutcome, String> {
    tokio::task::spawn_blocking(move || {
        if frames.is_empty() {
            return Err("the take contained no Audio Input frames".into());
        }
        let clip_name = format!("Recording {}", start_position_samples);
        let audio = Arc::new(DecodedAudio {
            channels: vec![
                frames.iter().map(|frame| frame[0]).collect(),
                frames.iter().map(|frame| frame[1]).collect(),
            ],
            sample_rate,
        });
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let temporary = std::env::temp_dir().join(format!(
            "vibez-recording-{}-{nonce}.wav",
            std::process::id()
        ));
        let result = (|| {
            vibez_audio_io::file_io::write_wav_file(&temporary, &audio)
                .map_err(|error| format!("WAV encoding failed: {error}"))?;
            let bytes = std::fs::read(&temporary)
                .map_err(|error| format!("recorded WAV could not be staged: {error}"))?;
            let source = vibez_project::project_format_v1::stage_local_content(
                Path::new("Recorded Audio"),
                &format!("{clip_name}.wav"),
                &bytes,
            )
            .map_err(|error| format!("Project Media staging failed: {error}"))?;
            Ok(AudioRecordingOutcome {
                track_id,
                start_position_samples,
                clip_name,
                audio,
                source,
            })
        })();
        let _ = std::fs::remove_file(temporary);
        result
    })
    .await
    .map_err(|error| format!("recording finalization task failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_core::track::MediaSourceRef;

    #[tokio::test]
    async fn completed_take_is_staged_before_it_can_become_project_content() {
        let outcome =
            finalize_audio_input_take(TrackId::new(), 9_600, 48_000, vec![[0.25, -0.25]; 480])
                .await
                .unwrap();

        assert_eq!(outcome.start_position_samples, 9_600);
        assert_eq!(outcome.audio.num_frames(), 480);
        match outcome.source {
            MediaSourceRef::StagedProjectMedia {
                staging_path,
                file_name,
                ..
            } => {
                assert!(staging_path.is_file());
                assert!(file_name.ends_with(".wav"));
            }
            source => panic!("expected staged Project Media, got {source:?}"),
        }
    }

    #[tokio::test]
    async fn recorded_take_reopens_from_project_media_at_the_same_position() {
        let directory = tempfile::tempdir().unwrap();
        let project_path = directory.path().join("recording-roundtrip.vzp");
        let track = vibez_core::track::TrackInfo::new("Vocal");
        let outcome = finalize_audio_input_take(track.id, 12_345, 48_000, vec![[0.4, -0.2]; 960])
            .await
            .unwrap();
        let project = vibez_project::Project {
            tracks: vec![track.clone()],
            arrange: vibez_project::TimelineInfo {
                clips: vec![vibez_core::track::ClipInfo {
                    id: ClipId::new(),
                    track_id: track.id,
                    name: outcome.clip_name,
                    position: outcome.start_position_samples,
                    source_offset: 0,
                    duration: outcome.audio.num_frames() as u64,
                    source: Some(outcome.source),
                    file_path: None,
                    loop_enabled: false,
                    loop_start: 0,
                    loop_end: outcome.audio.num_frames() as u64,
                    original_bpm: None,
                    warped: false,
                    warped_to_bpm: None,
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        vibez_project::project_format_v1::save_project_v1(&project_path, None, project).unwrap();

        let reopened = super::load_project_async(project_path, None).await.unwrap();
        assert_eq!(reopened.project.arrange.clips[0].position, 12_345);
        assert_eq!(reopened.clips[0].audio.num_frames(), 960);
        assert!(matches!(
            reopened.project.arrange.clips[0].source,
            Some(MediaSourceRef::ProjectMedia { .. })
        ));
    }
}
