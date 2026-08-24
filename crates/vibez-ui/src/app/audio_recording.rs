//! Arrange Audio Track Recording and live Resample lifecycle.

use std::sync::Arc;
use std::time::Instant;

use iced::Task;
use vibez_audio_io::audio_host::AudioBackend;
use vibez_audio_io::audio_input::{AudioInputEvent, AudioInputStream};
use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::TrackKind;
use vibez_core::track::{AudioInputRoute, InputMonitoring};
use vibez_engine::commands::EngineCommand;

use crate::domains::audio_recording::{AudioRecordingPhase, AudioRecordingSource};
use crate::domains::transport::TransportMsg;
use crate::message::{AudioRecordingOutcome, Message};
use crate::state::{ArrangementSelection, ProjectTrack, UiClip, Workspace};

use super::audio_take_finalization::{
    finalize_audio_take, recording_target_accepts_finalizer, FinalizeAudioTake,
};
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
                self.state.status_text = match track_recording_source_label(
                    self.state
                        .find_track(track_id)
                        .map(|track| track.audio_input_route),
                    &self.state,
                ) {
                    Some(source) => format!("{name} armed for {source}"),
                    None => format!("{name} armed"),
                };
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
        if !self.input_route_is_available(track_id, route) {
            self.state.status_text = "That Audio Track input is unavailable".into();
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
            if route.resample_source().is_some() {
                track.input_monitoring = InputMonitoring::Off;
            }
        }
        self.state.audio_recording.monitor_track = self.persisted_monitor_on_track();
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
        if self
            .state
            .find_track(track_id)
            .is_some_and(|track| track.audio_input_route.resample_source().is_some())
        {
            self.state.status_text =
                "Resample follows the source Track's audible output; input monitoring is not needed"
                    .into();
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
        if !matches!(self.state.audio_stream_health, AudioStreamHealth::Running) {
            self.state.status_text =
                "Audio Track Recording needs a running Audio Output clock".into();
            return Task::none();
        }
        let Some(track_id) = self.state.audio_recording.armed_track else {
            self.state.status_text = "Arm an Audio Project Track before recording".into();
            return Task::none();
        };
        let Some(source) = self.recording_source_for_target(track_id) else {
            self.state.status_text = "The armed Audio Track input is unavailable".into();
            return Task::none();
        };
        if source == AudioRecordingSource::HardwareInput && self._input_stream.is_none() {
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
        if !self.state.audio_recording.begin(start, source) {
            self.discard_audio_recording_transaction();
            return Task::none();
        }
        // A take is allowed to extend Arrange beyond the last existing Clip.
        // This must precede Play in the ordered engine queue so the first
        // output callback cannot immediately auto-stop at the old content end.
        self.send_command(EngineCommand::SetArrangementRecording(true));
        self.input_bridge.begin_recording();
        self.sync_audio_input_target();
        let track_name = self
            .state
            .find_track(track_id)
            .map_or("Audio Track", |track| track.name.as_str());
        self.state.status_text = match source {
            AudioRecordingSource::HardwareInput => {
                format!("Recording Audio Input into {track_name}")
            }
            AudioRecordingSource::TrackOutput(source_id) => {
                let source_name = self
                    .state
                    .find_track(source_id)
                    .map_or("MIDI Track", |track| track.name.as_str());
                format!("Resampling {source_name} into {track_name}")
            }
        };
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
        let truncated = self.state.audio_recording.truncated;
        let underrun_frames = self.input_bridge.underrun_frames();
        let source = self
            .state
            .audio_recording
            .source
            .unwrap_or(AudioRecordingSource::HardwareInput);
        let completion_label = match source {
            AudioRecordingSource::HardwareInput => "Recorded Audio Input".into(),
            AudioRecordingSource::TrackOutput(source_id) => {
                let source_name = self
                    .state
                    .find_track(source_id)
                    .map_or("MIDI Track", |track| track.name.as_str());
                format!("Resampled {source_name}")
            }
        };
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
            finalize_audio_take(FinalizeAudioTake {
                track_id,
                start_position_samples,
                sample_rate,
                frames,
                recording_source: source,
                completion_label,
                truncated,
                underrun_frames,
            }),
            Message::AudioRecordingFinalized,
        )
    }

    pub(super) fn finish_audio_recording(
        &mut self,
        result: Result<AudioRecordingOutcome, String>,
    ) -> Task<Message> {
        if self.state.audio_recording.phase != AudioRecordingPhase::Finalizing {
            // A project close/new/open invalidates its in-flight finalizer.
            // Its staged bytes remain disposable and are swept at startup.
            return Task::none();
        }
        match result {
            Ok(outcome) => {
                if !recording_target_accepts_finalizer(
                    self.state.audio_recording.phase,
                    self.state.audio_recording.armed_track,
                    outcome.track_id,
                    self.state.find_track(outcome.track_id).is_some(),
                ) {
                    self.state.audio_recording.finish();
                    self.discard_audio_recording_transaction();
                    self.sync_audio_input_target();
                    self.state.status_text =
                        "Recorded take was discarded because its target Track no longer exists"
                            .into();
                    return Task::none();
                }
                let clip_id = ClipId::new();
                let duration = outcome.audio.num_frames() as u64;
                let quality_warning = outcome.quality_warning.clone();
                self.send_command(EngineCommand::AddClip {
                    track_id: outcome.track_id,
                    clip_id,
                    audio: Arc::clone(&outcome.audio),
                    position: outcome.start_position_samples,
                    source_offset: 0,
                    start_marker: 0,
                    duration,
                    loop_enabled: false,
                    loop_start: 0,
                    loop_end: duration,
                    linear_gain: 1.0,
                    fades: Default::default(),
                    playback_direction: Default::default(),
                    warp_markers: Default::default(),
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
                        start_marker: 0,
                        duration,
                        loop_enabled: false,
                        loop_start: 0,
                        loop_end: duration,
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
                let completed = format!(
                    "{} · {:.2} s · one undo step",
                    outcome.completion_label,
                    duration as f64 / self.state.transport.sample_rate as f64,
                );
                self.state.status_text = quality_warning
                    .map(|warning| format!("{completed} · Warning: {warning}"))
                    .unwrap_or(completed);
                return self.schedule_auto_detect_clip_transients(
                    vibez_project::TimelineLocation::Arrange,
                    outcome.track_id,
                    clip_id,
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
        let target_missing = self
            .active_audio_input_target()
            .is_some_and(|id| self.state.find_track(id).is_none());
        if self.state.audio_recording.is_busy() && target_missing {
            return Some(self.abort_audio_recording(
                "Audio recording stopped — its target Track was removed. No Clip was created.",
            ));
        }
        let source_missing = matches!(
            self.state.audio_recording.source,
            Some(AudioRecordingSource::TrackOutput(source_id))
                if !self.state.find_track(source_id).is_some_and(ProjectTrack::is_playable_midi_target)
        );
        if self.state.audio_recording.is_busy() && source_missing {
            return Some(self.abort_audio_recording(
                "Resample stopped because its source MIDI Track was removed or became unavailable. No Clip was created.",
            ));
        }
        if !self.state.audio_recording.is_busy() && target_missing {
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
            let previous_len = self.state.audio_recording.captured_frames.len();
            self.input_bridge
                .drain_recorded(&mut self.state.audio_recording.captured_frames);
            self.state
                .audio_recording
                .captured_frames_appended(previous_len);
            if let Some(position) = self.input_bridge.record_start_position() {
                self.state.audio_recording.start_position_samples = position;
            }
            if self.input_bridge.overflowed() {
                self.state.audio_recording.mark_truncated();
                if self.state.audio_recording.is_recording() {
                    let task = self.stop_audio_recording();
                    self.state.status_text = if matches!(
                        self.state.audio_recording.source,
                        Some(AudioRecordingSource::TrackOutput(_))
                    ) {
                        "The bounded Resample buffer filled — salvaging the valid part of this take…"
                            .into()
                    } else {
                        "Audio clock drift filled the bounded input buffer — salvaging the valid part of this take…"
                            .into()
                    };
                    return Some(task);
                }
            }
        }
        if self.state.audio_recording.is_capturing()
            && matches!(self.state.audio_stream_health, AudioStreamHealth::Error(_))
        {
            return Some(self.abort_audio_recording(
                "Audio recording stopped because the Audio Output clock failed. No Clip was created.",
            ));
        }
        if let Some(error) = errors.into_iter().next() {
            self._input_stream = None;
            if self.state.audio_recording.is_capturing()
                && self.state.audio_recording.source == Some(AudioRecordingSource::HardwareInput)
            {
                return Some(self.abort_audio_recording(&format!(
                    "Audio Input disconnected — {error}. No Clip was created."
                )));
            }
            self.state.status_text = format!("Audio Input disconnected — {error}");
            return None;
        }
        if self.state.audio_recording.phase == AudioRecordingPhase::Stopping
            && self.input_bridge.recording_stopped()
        {
            return Some(self.finalize_stopped_audio_recording());
        }
        if self
            .state
            .audio_recording
            .stop_ack_timed_out(Instant::now())
        {
            return Some(self.abort_audio_recording(
                "Audio recording stopped because the Audio Output callback did not acknowledge Stop. No Clip was created.",
            ));
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
        let route = self
            .state
            .find_track(target.expect("checked target"))
            .map(|track| track.audio_input_route);
        if route.is_some_and(|route| route.resample_source().is_some()) {
            self._input_stream = None;
            return Ok(());
        }
        let requested_name = self.state.audio_settings.preferred_input_name.as_deref();
        let expected_device_name = self
            .state
            .audio_settings
            .selected_input_name()
            .map(str::to_owned);
        let must_reopen = input_stream_must_reopen(
            self._input_stream.as_ref().map(|stream| {
                (
                    stream.backend,
                    stream.device_name.as_str(),
                    stream.sample_rate,
                )
            }),
            self.state.audio_settings.backend,
            expected_device_name.as_deref(),
            self.state.transport.sample_rate,
        );
        if must_reopen {
            self._input_stream = None;
            let backend = self.state.audio_settings.backend;
            let stream = if backend == AudioBackend::Asio {
                let device = self
                    ._stream
                    .as_ref()
                    .and_then(|output| output.active_device())
                    .ok_or_else(|| "ASIO Audio Output is unavailable".to_string())?;
                AudioInputStream::open_on_device(
                    backend,
                    device,
                    self.state.transport.sample_rate,
                    Some(self.state.audio_settings.buffer_size),
                    Arc::clone(&self.input_bridge),
                )
            } else {
                AudioInputStream::open(
                    backend,
                    requested_name,
                    self.state.transport.sample_rate,
                    Some(self.state.audio_settings.buffer_size),
                    Arc::clone(&self.input_bridge),
                )
            }
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

    pub(super) fn persisted_monitor_on_track(&self) -> Option<TrackId> {
        self.state
            .project_tracks
            .tracks
            .iter()
            .find(|track| {
                track.kind == TrackKind::Audio
                    && track.audio_input_route.is_hardware()
                    && track.input_monitoring == InputMonitoring::On
            })
            .map(|track| track.id)
    }

    fn sync_audio_input_target(&self) {
        self.input_bridge.set_target(None, false);
        self.input_bridge.set_resample_source(None);
        let Some(track_id) = self.active_audio_input_target() else {
            return;
        };
        let Some(track) = self.state.find_track(track_id) else {
            return;
        };
        if let Some(source_id) = track.audio_input_route.resample_source() {
            self.input_bridge.set_resample_source(
                self.state
                    .find_track(source_id)
                    .is_some_and(ProjectTrack::is_playable_midi_target)
                    .then_some(source_id),
            );
            return;
        }
        let monitoring = match track.input_monitoring {
            InputMonitoring::Off => false,
            InputMonitoring::Auto => self.state.audio_recording.armed_track == Some(track_id),
            InputMonitoring::On => true,
        };
        self.input_bridge.set_route(track.audio_input_route);
        self.input_bridge.set_target(Some(track_id), monitoring);
    }

    fn input_route_is_available(&self, target_id: TrackId, route: AudioInputRoute) -> bool {
        match route {
            AudioInputRoute::Mono { .. } | AudioInputRoute::Stereo { .. } => {
                route_fits_channels(route, self.state.audio_settings.input_channel_count())
            }
            AudioInputRoute::Resample { track_id } => {
                track_id != target_id
                    && self
                        .state
                        .find_track(track_id)
                        .is_some_and(ProjectTrack::is_playable_midi_target)
            }
        }
    }

    fn recording_source_for_target(&self, target_id: TrackId) -> Option<AudioRecordingSource> {
        let route = self.state.find_track(target_id)?.audio_input_route;
        match route {
            AudioInputRoute::Mono { .. } | AudioInputRoute::Stereo { .. } => self
                .input_route_is_available(target_id, route)
                .then_some(AudioRecordingSource::HardwareInput),
            AudioInputRoute::Resample { track_id } => self
                .input_route_is_available(target_id, route)
                .then_some(AudioRecordingSource::TrackOutput(track_id)),
        }
    }

    fn abort_audio_recording(&mut self, status: &str) -> Task<Message> {
        self.input_bridge.end_recording();
        self.state.audio_recording.captured_frames.clear();
        self.state.audio_recording.finish();
        self.discard_audio_recording_transaction();
        self.sync_audio_input_target();
        self.state.status_text = status.into();
        if self.state.transport.playing {
            self.update(Message::Transport(TransportMsg::Stop))
        } else {
            Task::none()
        }
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
        AudioInputRoute::Resample { .. } => false,
    }
}

fn track_recording_source_label(
    route: Option<AudioInputRoute>,
    state: &crate::state::AppState,
) -> Option<String> {
    match route? {
        AudioInputRoute::Mono { .. } | AudioInputRoute::Stereo { .. } => Some("Audio Input".into()),
        AudioInputRoute::Resample { track_id } => state
            .find_track(track_id)
            .map(|track| format!("Resample · {}", track.name)),
    }
}

fn input_stream_must_reopen(
    current: Option<(AudioBackend, &str, u32)>,
    expected_backend: AudioBackend,
    expected_device_name: Option<&str>,
    expected_sample_rate: u32,
) -> bool {
    current.is_none_or(|(backend, device_name, sample_rate)| {
        backend != expected_backend
            || sample_rate != expected_sample_rate
            || expected_device_name.is_none_or(|expected| device_name != expected)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_from_a_named_input_to_a_different_system_default_reopens() {
        assert!(input_stream_must_reopen(
            Some((AudioBackend::System, "USB Interface", 48_000)),
            AudioBackend::System,
            Some("Built-in Audio"),
            48_000,
        ));
        assert!(!input_stream_must_reopen(
            Some((AudioBackend::System, "Built-in Audio", 48_000)),
            AudioBackend::System,
            Some("Built-in Audio"),
            48_000,
        ));
        assert!(input_stream_must_reopen(
            Some((AudioBackend::System, "Built-in Audio", 48_000)),
            AudioBackend::Asio,
            Some("Built-in Audio"),
            48_000,
        ));
    }
}
