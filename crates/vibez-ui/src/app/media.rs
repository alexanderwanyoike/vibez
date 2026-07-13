//! Split out of app.rs; inherent methods on [`super::App`].

use std::path::PathBuf;
use std::sync::Arc;

use iced::Task;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::{InstrumentKind, TrackKind};
use vibez_core::track::MediaSourceRef;
use vibez_dropbox::DropboxEntry;
use vibez_engine::commands::{AuditionSync, EngineCommand};

use crate::message::{BrowserImportTarget, Message};
use crate::state::{AuditionMode, SampleBrowserEntry, UiClip, UiDrumPad, UiTrack};

use super::*;

impl App {
    pub(super) fn stop_browser_audition(&mut self) {
        self.send_command(EngineCommand::StopAudition);
        self.state.browser.stop_audition_state();
    }

    pub(super) fn start_browser_audition(&mut self, audio: Arc<DecodedAudio>) {
        let queued =
            self.state.transport.playing && self.state.browser.audition_sync != AuditionSync::Off;
        self.send_command(EngineCommand::StartAudition {
            audio,
            sync: self.state.browser.audition_sync,
            looped: self.state.browser.audition_loop,
        });
        self.state.browser.mark_audition_requested(queued);
        let mode = match self.state.browser.audition_mode {
            AuditionMode::Raw => "RAW",
            AuditionMode::Warp => "WARP",
        };
        self.state.status_text = if queued {
            format!("{mode} Audition queued")
        } else {
            format!("{mode} Audition playing")
        };
    }

    pub(super) fn schedule_browser_bpm_detection(
        &mut self,
        source: MediaSourceRef,
        audio: Arc<DecodedAudio>,
    ) -> Task<Message> {
        if !self.state.browser.begin_bpm_detection(&source) {
            return Task::none();
        }
        let sample_rate = audio.sample_rate;
        Task::perform(detect_clip_bpm_async(audio, sample_rate), move |estimate| {
            Message::BrowserBpmDetected(
                source.clone(),
                estimate.map(|value| (value.bpm, value.confidence)),
            )
        })
    }

    pub(super) fn prepare_browser_warp(
        &mut self,
        source: MediaSourceRef,
        raw: Arc<DecodedAudio>,
        source_bpm: f64,
    ) -> Task<Message> {
        let project_bpm = self.state.transport.bpm;
        self.state.browser.begin_audition_load(&source);
        self.state.status_text = format!("Preparing WARP at {source_bpm:.1} BPM...");
        Task::perform(
            warp_browser_audition_async(raw, source_bpm, project_bpm),
            move |result| Message::BrowserAuditionWarpReady {
                source: source.clone(),
                source_bpm,
                project_bpm,
                result,
            },
        )
    }

    pub(super) fn play_browser_mode(
        &mut self,
        source: MediaSourceRef,
        raw: Arc<DecodedAudio>,
    ) -> Task<Message> {
        let detection = self.schedule_browser_bpm_detection(source.clone(), Arc::clone(&raw));
        match self.state.browser.audition_mode {
            AuditionMode::Raw => {
                self.start_browser_audition(raw);
                detection
            }
            AuditionMode::Warp => {
                self.stop_browser_audition();
                if let Some(source_bpm) = self.state.browser.audition_bpm_confirmed {
                    Task::batch([
                        detection,
                        self.prepare_browser_warp(source, raw, source_bpm),
                    ])
                } else {
                    self.state.status_text = if self.state.browser.audition_bpm_detecting {
                        "Detecting source BPM; WARP awaits confirmation".into()
                    } else {
                        "Confirm or enter a positive source BPM for WARP".into()
                    };
                    detection
                }
            }
        }
    }

    pub(super) fn selected_sample_browser_entry(&self) -> Option<&SampleBrowserEntry> {
        let selected = self.state.browser.selected_source.as_ref()?;
        self.state
            .browser
            .entries
            .iter()
            .find(|entry| &entry.source == selected)
    }

    pub(super) fn selected_browser_device_target(&self) -> Option<BrowserImportTarget> {
        let track = self
            .state
            .arrangement
            .selected_track
            .and_then(|track_id| self.state.find_track(track_id))?;
        match track.instrument_kind {
            Some(InstrumentKind::Sampler) => Some(BrowserImportTarget::Sampler(track.id)),
            Some(InstrumentKind::DrumRack) => Some(BrowserImportTarget::DrumRackPad {
                track_id: track.id,
                pad_index: track
                    .selected_drum_pad
                    .min(track.drum_rack_pads.len().saturating_sub(1)),
            }),
            _ => None,
        }
    }

    pub(super) fn sync_drum_rack_pad_state(&mut self, track_id: TrackId, pad_index: usize) {
        let state = self
            .state
            .find_track(track_id)
            .and_then(|track| track.drum_rack_pads.get(pad_index))
            .map(UiDrumPad::to_state);
        if let Some(state) = state {
            self.send_command(EngineCommand::SetDrumRackPadState {
                track_id,
                pad_index,
                state,
            });
        }
    }

    pub(super) fn apply_sampler_sample_loaded(
        &mut self,
        track_id: TrackId,
        audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
        name: String,
        source: MediaSourceRef,
    ) {
        if let Some(track) = self.state.find_track_mut(track_id) {
            track.sample_name = Some(name.clone());
            track.sample_source = Some(source.clone());
            track.sample_audio = Some(Arc::clone(&audio));
        }
        self.send_command(EngineCommand::LoadSamplerSample {
            track_id,
            sample: audio,
            sample_name: name.clone(),
        });
        self.state.status_text = format!("Loaded sample: {name}");
    }

    pub(super) fn apply_drum_rack_pad_loaded(
        &mut self,
        track_id: TrackId,
        pad_index: usize,
        audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
        name: String,
        source: MediaSourceRef,
    ) {
        if let Some(track) = self.state.find_track_mut(track_id) {
            if let Some(pad) = track.drum_rack_pads.get_mut(pad_index) {
                pad.name = Some(name.clone());
                pad.source = Some(source.clone());
                pad.audio = Some(Arc::clone(&audio));
            }
        }
        self.sync_drum_rack_pad_state(track_id, pad_index);
        self.send_command(EngineCommand::LoadDrumRackPadSample {
            track_id,
            pad_index,
            sample: audio,
            sample_name: name.clone(),
        });
        self.state.status_text = format!("Loaded pad {}: {name}", pad_index + 1);
    }

    pub(super) fn ensure_audio_track_for_import(&mut self, preferred: Option<TrackId>) -> TrackId {
        if let Some(track_id) = preferred {
            if self
                .state
                .find_track(track_id)
                .is_some_and(|track| matches!(track.kind, TrackKind::Audio))
            {
                return track_id;
            }
        }

        let track_num = self.next_unique_track_number("Audio");
        self.state.arrangement.next_track_number = track_num + 1;
        let id = TrackId::new();
        let color_index = ((track_num - 1) % 8) as u8;
        let name = format!("Audio {track_num}");

        self.send_command(EngineCommand::AddTrack(id, name.clone()));
        self.state
            .arrangement
            .tracks
            .push(UiTrack::new(id, name, color_index));
        self.state.arrangement.selected_track = Some(id);
        id
    }

    pub(super) fn add_audio_clip_to_track(
        &mut self,
        track_id: TrackId,
        audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
        name: String,
        source: MediaSourceRef,
    ) -> Task<Message> {
        let clip_id = ClipId::new();
        let existing_end = self
            .state
            .find_track(track_id)
            .map(|track| {
                track
                    .clips
                    .iter()
                    .map(|clip| clip.position.saturating_add(clip.duration))
                    .max()
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        let duration = audio.num_frames() as u64;

        self.send_command(EngineCommand::AddClip {
            track_id,
            clip_id,
            audio: Arc::clone(&audio),
            position: existing_end,
            source_offset: 0,
            duration,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        });

        if let Some(track) = self.state.find_track_mut(track_id) {
            track.clips.push(UiClip {
                id: clip_id,
                name: name.clone(),
                audio: Arc::clone(&audio),
                source: Some(source),
                position: existing_end,
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
        self.state.status_text = format!("Added clip: {name}");
        self.schedule_auto_warp_if_enabled(track_id, clip_id, audio)
    }

    pub(super) fn apply_browser_sample_decoded(
        &mut self,
        target: BrowserImportTarget,
        audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
        name: String,
        source: MediaSourceRef,
    ) -> Task<Message> {
        match target {
            BrowserImportTarget::ArrangementClip(preferred_track) => {
                let track_id = self.ensure_audio_track_for_import(preferred_track);
                self.add_audio_clip_to_track(track_id, audio, name, source)
            }
            BrowserImportTarget::ArrangementClipAt {
                track_id,
                position_samples,
            } => self.add_audio_clip_to_track_at(track_id, position_samples, audio, name, source),
            BrowserImportTarget::Sampler(track_id) => {
                self.apply_sampler_sample_loaded(track_id, audio, name, source);
                Task::none()
            }
            BrowserImportTarget::DrumRackPad {
                track_id,
                pad_index,
            } => {
                self.apply_drum_rack_pad_loaded(track_id, pad_index, audio, name, source);
                Task::none()
            }
        }
    }

    pub(super) fn add_audio_clip_to_track_at(
        &mut self,
        track_id: TrackId,
        position_samples: u64,
        audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
        name: String,
        source: MediaSourceRef,
    ) -> Task<Message> {
        // Guard: if the target is not an audio track, refuse rather than
        // silently redirecting the drop. Prevents the "clip lands on the
        // wrong lane" surprise.
        let track_name = match self.state.find_track(track_id) {
            Some(t) if matches!(t.kind, TrackKind::Audio) => t.name.clone(),
            Some(t) => {
                self.state.status_text = format!(
                    "Can't drop audio on non-audio track '{}'; drag to an audio lane.",
                    t.name
                );
                return Task::none();
            }
            None => {
                self.state.status_text = "Drop target not found; drag cancelled".to_string();
                return Task::none();
            }
        };

        let clip_id = ClipId::new();
        let duration = audio.num_frames() as u64;

        self.send_command(EngineCommand::AddClip {
            track_id,
            clip_id,
            audio: Arc::clone(&audio),
            position: position_samples,
            source_offset: 0,
            duration,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        });
        if let Some(track) = self.state.find_track_mut(track_id) {
            track.clips.push(UiClip {
                id: clip_id,
                name: name.clone(),
                audio: Arc::clone(&audio),
                source: Some(source),
                position: position_samples,
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
        self.state.status_text = format!("Dropped '{name}' on {track_name}");
        self.schedule_auto_warp_if_enabled(track_id, clip_id, audio)
    }

    pub(super) fn dispatch_drop_on_arrangement(
        &mut self,
        track_id: TrackId,
        position_samples: u64,
        source: MediaSourceRef,
    ) -> Task<Message> {
        let target = BrowserImportTarget::ArrangementClipAt {
            track_id,
            position_samples,
        };
        self.dispatch_drop_for_target(source, target)
    }

    pub(super) fn dispatch_drop_for_target(
        &mut self,
        source: MediaSourceRef,
        target: BrowserImportTarget,
    ) -> Task<Message> {
        match source {
            MediaSourceRef::LocalFile { path } => {
                let name = path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                let ret_source = MediaSourceRef::LocalFile { path: path.clone() };
                self.state.status_text = format!("Dropping {name}...");
                Task::perform(decode_file_async(path), move |result| match result {
                    Ok(audio) => Message::BrowserSampleDecoded(
                        target.clone(),
                        Arc::new(audio),
                        name.clone(),
                        ret_source.clone(),
                    ),
                    Err(err) => Message::BrowserSampleDecodeError(err),
                })
            }
            MediaSourceRef::DropboxFile {
                path_lower,
                display_path,
                rev,
            } => {
                let Some(client) = self.dropbox_client.clone() else {
                    self.state.status_text = "Not connected to Dropbox for this drop".to_string();
                    return Task::none();
                };
                let cache = self.dropbox_cache.clone();
                let name = display_path
                    .rsplit_once('/')
                    .map(|(_, n)| n.to_string())
                    .unwrap_or_else(|| display_path.clone());
                let entry = DropboxEntry {
                    path_lower,
                    path_display: display_path,
                    name: name.clone(),
                    is_folder: false,
                    rev,
                    size: None,
                };
                self.state.status_text = format!("Dropping {name}...");
                Task::perform(
                    fetch_dropbox_sample_async(client, cache, entry),
                    move |result| match result {
                        Ok((audio, decoded_name, source)) => Message::BrowserSampleDecoded(
                            target.clone(),
                            audio,
                            decoded_name,
                            source,
                        ),
                        Err(err) => Message::BrowserSampleDecodeError(err),
                    },
                )
            }
        }
    }

    pub(super) fn dispatch_audio_quantize(
        &mut self,
        track_id: TrackId,
        clip_id: ClipId,
        grid: crate::state::SnapGrid,
    ) -> Task<Message> {
        let Some(track) = self.state.find_track(track_id) else {
            self.state.status_text = "Track not found".to_string();
            return Task::none();
        };
        let Some(clip) = track.clips.iter().find(|c| c.id == clip_id) else {
            self.state.status_text = "Clip not found".to_string();
            return Task::none();
        };
        if self.state.transport.bpm <= 0.0 || self.state.transport.sample_rate == 0 {
            self.state.status_text = "Cannot quantize at zero BPM".to_string();
            return Task::none();
        }

        let input = QuantizeInput {
            audio: Arc::clone(&clip.audio),
            bpm: self.state.transport.bpm,
            sample_rate: self.state.transport.sample_rate,
            grid,
            clip_position: clip.position,
            clip_source_offset: clip.source_offset,
            clip_duration: clip.duration,
            original_name: clip.name.clone(),
            new_clip_id: ClipId::new(),
        };

        self.state.status_text = format!("Quantizing {}...", input.original_name);
        Task::perform(quantize_audio_clip_async(input), move |result| {
            Message::AudioQuantizeReady {
                track_id,
                old_clip_id: clip_id,
                result,
            }
        })
    }

    pub(super) fn dispatch_detect_clip_bpm(
        &mut self,
        track_id: TrackId,
        clip_id: ClipId,
    ) -> Task<Message> {
        let Some(track) = self.state.find_track(track_id) else {
            self.state.status_text = "Track not found".to_string();
            return Task::none();
        };
        let Some(clip) = track.clips.iter().find(|c| c.id == clip_id) else {
            self.state.status_text = "Clip not found".to_string();
            return Task::none();
        };
        // Always detect against the un-warped audio so the result is
        // the sample's intrinsic tempo, not the warped-to-project tempo.
        let audio = clip
            .original_audio
            .clone()
            .unwrap_or_else(|| Arc::clone(&clip.audio));
        let sample_rate = self.state.transport.sample_rate;
        self.state.status_text = format!("Detecting BPM for {}...", clip.name);
        Task::perform(detect_clip_bpm_async(audio, sample_rate), move |estimate| {
            Message::ClipBpmDetected {
                track_id,
                clip_id,
                bpm: estimate.map(|e| e.bpm),
                confidence: estimate.map(|e| e.confidence).unwrap_or(0.0),
            }
        })
    }

    /// Ableton-style global tempo follow. Warped audio clips keep
    /// their BAR position (sample positions rescale by the tempo
    /// ratio) and their audio re-stretches to the new tempo through
    /// the idempotent re-warp path. Unwarped audio clips keep
    /// absolute time, exactly like unwarped clips in Ableton. MIDI
    /// clips are beat-positioned and follow inherently.
    pub(super) fn follow_tempo_change(&mut self, old_bpm: f64, new_bpm: f64) -> Task<Message> {
        let position_ratio = old_bpm / new_bpm;
        let mut warped: Vec<(TrackId, ClipId)> = Vec::new();
        let mut moves: Vec<(TrackId, ClipId, u64)> = Vec::new();

        for track in &mut self.state.arrangement.tracks {
            for clip in &mut track.clips {
                if !clip.warped {
                    continue;
                }
                let new_position = (clip.position as f64 * position_ratio).round() as u64;
                if new_position != clip.position {
                    clip.position = new_position;
                    moves.push((track.id, clip.id, new_position));
                }
                warped.push((track.id, clip.id));
            }
        }
        for (track_id, clip_id, new_position) in moves {
            self.send_command(EngineCommand::MoveClip {
                track_id,
                clip_id,
                new_position,
            });
        }
        if warped.is_empty() {
            return Task::none();
        }
        self.state.status_text = format!(
            "Tempo {old_bpm:.0} -> {new_bpm:.0}: re-warping {} clip(s)",
            warped.len()
        );
        let tasks: Vec<Task<Message>> = warped
            .into_iter()
            .map(|(track_id, clip_id)| self.dispatch_warp_clip_to_project(track_id, clip_id))
            .collect();
        Task::batch(tasks)
    }

    pub(super) fn dispatch_warp_clip_to_project(
        &mut self,
        track_id: TrackId,
        clip_id: ClipId,
    ) -> Task<Message> {
        let project_bpm = self.state.transport.bpm;
        let sample_rate = self.state.transport.sample_rate;
        if project_bpm <= 0.0 || sample_rate == 0 {
            self.state.status_text = "Cannot warp at zero BPM".to_string();
            return Task::none();
        }
        let Some(track) = self.state.find_track(track_id) else {
            self.state.status_text = "Track not found".to_string();
            return Task::none();
        };
        let Some(clip) = track.clips.iter().find(|c| c.id == clip_id) else {
            self.state.status_text = "Clip not found".to_string();
            return Task::none();
        };
        let Some(clip_bpm) = clip.original_bpm else {
            self.state.status_text = "Set or detect the clip's BPM before warping".to_string();
            return Task::none();
        };
        if clip_bpm <= 0.0 {
            self.state.status_text = "Invalid BPM".to_string();
            return Task::none();
        }
        // If the clip was already warped once, warp the retained
        // original_audio. Otherwise the current `audio` is itself the
        // original. Either way the clip's geometry fields are in
        // samples of the CURRENT buffer, so `fields_frames` must be
        // the current buffer's frame count for the rescale to be
        // idempotent across repeated warps.
        let source_audio = clip
            .original_audio
            .clone()
            .unwrap_or_else(|| Arc::clone(&clip.audio));
        let input = crate::warp::WarpClipInput {
            audio: source_audio,
            fields_frames: clip.audio.num_frames() as u64,
            source_offset: clip.source_offset,
            duration: clip.duration,
            loop_start: clip.loop_start,
            loop_end: clip.loop_end,
            clip_bpm,
            project_bpm,
        };
        let _ = sample_rate;
        self.state.status_text = format!("Warping to {project_bpm:.0} BPM...");
        Task::perform(crate::warp::warp_clip_async(input), move |result| {
            Message::ClipWarpReady {
                track_id,
                clip_id,
                result,
            }
        })
    }

    /// If auto-warp-on-import is enabled, return a background task
    /// that detects the imported clip's BPM and warps it to the
    /// project tempo. Call this right after a clip is inserted into
    /// state / the engine. The caller propagates the Task to the
    /// iced runtime (helpers return it up through
    /// `apply_browser_sample_decoded`).
    pub(super) fn schedule_auto_warp_if_enabled(
        &self,
        track_id: TrackId,
        clip_id: ClipId,
        audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
    ) -> Task<Message> {
        if !self.state.auto_warp_on_import
            || self.state.transport.bpm <= 0.0
            || self.state.transport.sample_rate == 0
        {
            return Task::none();
        }
        let input = AutoWarpInput {
            audio,
            sample_rate: self.state.transport.sample_rate,
            project_bpm: self.state.transport.bpm,
            confidence_threshold: self.state.warp_confidence_threshold,
        };
        Task::perform(auto_warp_clip_async(input), move |outcome| {
            Message::ClipAutoWarpReady {
                track_id,
                clip_id,
                outcome,
            }
        })
    }

    pub(super) fn selected_dropbox_entry(&self) -> Option<DropboxEntry> {
        let selected = self.state.browser.dropbox.selected_path.as_ref()?;
        for entries in self.state.browser.dropbox.folders.values() {
            if let Some(entry) = entries.iter().find(|e| &e.path_lower == selected) {
                return Some(entry.clone());
            }
        }
        None
    }

    pub(super) fn handle_add_clip_to_track(&mut self, track_id: TrackId) -> Task<Message> {
        // Guard: only audio tracks can have audio clips
        if let Some(track) = self.state.find_track(track_id) {
            if track.kind.is_midi() {
                self.state.status_text = "MIDI tracks use note clips, not audio".to_string();
                return Task::none();
            }
        }
        Task::perform(
            async {
                let handle = rfd::AsyncFileDialog::new()
                    .set_title("Add Audio Clip")
                    .add_filter("Audio", &["wav", "mp3", "flac", "ogg"])
                    .pick_file()
                    .await;
                handle.map(|h| h.path().to_path_buf())
            },
            move |path| Message::ClipFileSelected(track_id, path),
        )
    }

    pub(super) fn handle_clip_file_selected(
        &mut self,
        track_id: TrackId,
        path: Option<PathBuf>,
    ) -> Task<Message> {
        if let Some(path) = path {
            let file_name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            self.state.status_text = format!("Loading {file_name}...");
            let clip_id = ClipId::new();
            let source = MediaSourceRef::LocalFile { path: path.clone() };

            return Task::perform(decode_file_async(path), move |result| match result {
                Ok(audio) => Message::ClipAudioDecoded(
                    track_id,
                    clip_id,
                    Arc::new(audio),
                    file_name.clone(),
                    source.clone(),
                ),
                Err(e) => Message::ClipDecodeError(track_id, e),
            });
        }
        Task::none()
    }

    pub(super) fn handle_clip_audio_decoded(
        &mut self,
        track_id: TrackId,
        clip_id: ClipId,
        audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
        name: String,
        source: MediaSourceRef,
    ) -> Task<Message> {
        let existing_end = self
            .state
            .find_track(track_id)
            .map(|t| {
                t.clips
                    .iter()
                    .map(|c| c.position.saturating_add(c.duration))
                    .max()
                    .unwrap_or(0)
            })
            .unwrap_or(0);

        let duration = audio.num_frames() as u64;

        self.send_command(EngineCommand::AddClip {
            track_id,
            clip_id,
            audio: Arc::clone(&audio),
            position: existing_end,
            source_offset: 0,
            duration,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        });

        if let Some(track) = self.state.find_track_mut(track_id) {
            track.clips.push(UiClip {
                id: clip_id,
                name: name.clone(),
                audio,
                source: Some(source),
                position: existing_end,
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

        self.state.status_text = format!("Added clip: {name}");
        Task::none()
    }

    pub(super) fn handle_drum_rack_pad_file_selected(
        &mut self,
        track_id: TrackId,
        pad_index: usize,
        path: Option<PathBuf>,
    ) -> Task<Message> {
        if let Some(path) = path {
            let file_name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            self.state.status_text = format!("Loading {file_name}...");
            let source = MediaSourceRef::LocalFile { path: path.clone() };

            return Task::perform(decode_file_async(path), move |result| match result {
                Ok(audio) => Message::DrumRackPadSampleDecoded(
                    track_id,
                    pad_index,
                    Arc::new(audio),
                    file_name.clone(),
                    source.clone(),
                ),
                Err(e) => Message::DrumRackPadDecodeError(track_id, pad_index, e),
            });
        }
        Task::none()
    }

    pub(super) fn handle_drum_rack_pad_decode_error(
        &mut self,
        track_id: TrackId,
        _pad_index: usize,
        err: String,
    ) -> Task<Message> {
        self.state.arrangement.selected_track = Some(track_id);
        self.state.status_text = format!("Drum pad load error: {err}");
        Task::none()
    }

    pub(super) fn handle_rewarp_all_clips(&mut self) -> Task<Message> {
        // Collect targets first so we don't hold a borrow across dispatch.
        let targets: Vec<(TrackId, ClipId)> = self
            .state
            .arrangement
            .tracks
            .iter()
            .flat_map(|track| {
                track
                    .clips
                    .iter()
                    .filter(|c| c.warped && c.original_bpm.is_some())
                    .map(move |c| (track.id, c.id))
            })
            .collect();
        if targets.is_empty() {
            self.state.status_text = "Re-warp all: no warped clips to re-warp".to_string();
            return Task::none();
        }
        let count = targets.len();
        let tasks: Vec<Task<Message>> = targets
            .into_iter()
            .map(|(tid, cid)| self.dispatch_warp_clip_to_project(tid, cid))
            .collect();
        self.state.status_text = format!(
            "Re-warping {count} clip(s) to {:.0} BPM",
            self.state.transport.bpm
        );
        Task::batch(tasks)
    }

    pub(super) fn handle_drop_sample_on_drum_pad(
        &mut self,
        track_id: TrackId,
        pad_index: usize,
    ) -> Task<Message> {
        match self.state.browser.drag_source.take() {
            Some(source) => {
                self.state.browser.drag_label = None;
                self.dispatch_drop_for_target(
                    source,
                    BrowserImportTarget::DrumRackPad {
                        track_id,
                        pad_index,
                    },
                )
            }
            None => {
                // No active drag: treat release as a click.
                // Select the pad AND audition its loaded sample
                // via the engine's Audition Bus (bypasses
                // transport + mute + solo; one-shot). This is
                // the fastest way to hear what's on a pad
                // without drawing notes into the piano roll.
                let audition = self
                    .state
                    .find_track(track_id)
                    .and_then(|track| track.drum_rack_pads.get(pad_index))
                    .and_then(|pad| {
                        pad.audio.as_ref().map(|audio| {
                            (
                                Arc::clone(audio),
                                pad.name.clone().unwrap_or_else(|| "sample".into()),
                            )
                        })
                    });
                if let Some((audio, name)) = audition {
                    self.send_command(EngineCommand::StartAudition {
                        audio,
                        sync: AuditionSync::Off,
                        looped: false,
                    });
                    self.state.status_text = format!("Pad {}: {}", pad_index + 1, name);
                }
                self.update(Message::select_drum_rack_pad(track_id, pad_index))
            }
        }
    }

    pub(super) fn handle_import_selected_browser_sample_to_arrangement(&mut self) -> Task<Message> {
        if let Some(entry) = self.selected_sample_browser_entry().cloned() {
            let target = BrowserImportTarget::ArrangementClip(
                self.state.arrangement.selected_track.filter(|track_id| {
                    self.state
                        .find_track(*track_id)
                        .is_some_and(|track| matches!(track.kind, TrackKind::Audio))
                }),
            );
            if let MediaSourceRef::LocalFile { path } = &entry.source {
                let source = entry.source.clone();
                let name = entry.name.clone();
                self.state.status_text = format!("Loading {name}...");
                return Task::perform(
                    decode_file_async(path.clone()),
                    move |result| match result {
                        Ok(audio) => Message::BrowserSampleDecoded(
                            target.clone(),
                            Arc::new(audio),
                            name.clone(),
                            source.clone(),
                        ),
                        Err(err) => Message::BrowserSampleDecodeError(err),
                    },
                );
            }
        }
        Task::none()
    }

    pub(super) fn handle_load_selected_browser_sample_to_device(&mut self) -> Task<Message> {
        let Some(entry) = self.selected_sample_browser_entry().cloned() else {
            return Task::none();
        };
        let Some(target) = self.selected_browser_device_target() else {
            self.state.status_text =
                "Select a sampler or drum rack track to load from the browser".to_string();
            return Task::none();
        };
        if let MediaSourceRef::LocalFile { path } = &entry.source {
            let source = entry.source.clone();
            let name = entry.name.clone();
            self.state.status_text = format!("Loading {name}...");
            return Task::perform(
                decode_file_async(path.clone()),
                move |result| match result {
                    Ok(audio) => Message::BrowserSampleDecoded(
                        target.clone(),
                        Arc::new(audio),
                        name.clone(),
                        source.clone(),
                    ),
                    Err(err) => Message::BrowserSampleDecodeError(err),
                },
            );
        }
        Task::none()
    }
}
