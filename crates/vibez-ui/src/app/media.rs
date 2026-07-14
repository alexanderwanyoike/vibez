//! Split out of app.rs; inherent methods on [`super::App`].

use std::path::PathBuf;
use std::sync::Arc;

use iced::widget::scrollable;
use iced::Task;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::{InstrumentKind, TrackKind};
use vibez_core::track::MediaSourceRef;
use vibez_dropbox::DropboxEntry;
use vibez_engine::commands::{AuditionSync, EngineCommand};

use crate::message::{BrowserImportTarget, Message, PreparedBrowserImport};
use crate::state::{AuditionMode, SampleBrowserEntry, UiClip, UiDrumPad, UiTrack};

use super::*;

fn adjacent_result_index(length: usize, current: Option<usize>, direction: i8) -> Option<usize> {
    if length == 0 || direction == 0 {
        return None;
    }
    Some(if direction < 0 {
        current.unwrap_or(length).saturating_sub(1)
    } else {
        current
            .map(|index| (index + 1).min(length - 1))
            .unwrap_or(0)
    })
}

fn browser_results_scroll_offset(row_index: usize, visible_rows: usize) -> f32 {
    if visible_rows <= 1 {
        0.0
    } else {
        row_index.min(visible_rows - 1) as f32 / (visible_rows - 1) as f32
    }
}

pub(super) fn browser_results_scroll_id(mode: crate::state::SampleBrowserMode) -> scrollable::Id {
    let id = match mode {
        crate::state::SampleBrowserMode::Local => "browser-local-results",
        crate::state::SampleBrowserMode::Remote => "browser-remote-results",
    };
    scrollable::Id::new(id)
}

struct BrowserNavigableResult {
    source: MediaSourceRef,
    remote: Option<crate::remote_provider::RemoteCatalogEntry>,
    row_index: usize,
    visible_rows: usize,
}

impl App {
    pub(super) fn select_adjacent_browser_result(&mut self, direction: i8) -> Task<Message> {
        if !self.state.browser.open || direction == 0 {
            return Task::none();
        }
        let browser = &self.state.browser;
        let query = browser.search.trim().to_ascii_lowercase();
        let searching = !query.is_empty();
        let mode = browser.mode;
        let results: Vec<BrowserNavigableResult> = match mode {
            crate::state::SampleBrowserMode::Local => {
                let root_results = if !searching && browser.current_folder.is_none() {
                    browser.roots.len()
                } else if searching && browser.search_scope_path().is_none() {
                    browser
                        .roots
                        .iter()
                        .filter(|root| root.display().to_string().to_lowercase().contains(&query))
                        .count()
                } else {
                    0
                };
                let folder_results = browser
                    .folders
                    .iter()
                    .filter(|folder| browser.local_folder_is_result(folder, &query))
                    .count();
                let media_limit = browser
                    .results_visible_limit
                    .saturating_sub(root_results + folder_results);
                let mut entries: Vec<_> = browser
                    .entries
                    .iter()
                    .filter(|entry| browser.local_entry_is_result(entry, &query))
                    .collect();
                entries.sort_by(|left, right| {
                    (left.name.to_lowercase(), &left.relative_path)
                        .cmp(&(right.name.to_lowercase(), &right.relative_path))
                });
                let prefix_rows =
                    (root_results + folder_results).min(browser.results_visible_limit);
                let entries: Vec<_> = entries.into_iter().take(media_limit).collect();
                let visible_rows = prefix_rows + entries.len();
                entries
                    .into_iter()
                    .enumerate()
                    .map(|(index, entry)| BrowserNavigableResult {
                        source: entry.source.clone(),
                        remote: None,
                        row_index: prefix_rows + index,
                        visible_rows,
                    })
                    .collect()
            }
            crate::state::SampleBrowserMode::Remote => {
                let current = browser.remote.current_path.as_str();
                let in_current_tree = |entry: &crate::remote_provider::RemoteCatalogEntry| {
                    current.is_empty()
                        || entry.provider_item_id == current
                        || entry
                            .provider_item_id
                            .strip_prefix(current)
                            .is_some_and(|rest| rest.starts_with('/'))
                };
                let mut remote: Vec<_> = if searching {
                    browser
                        .remote
                        .catalog
                        .entries
                        .iter()
                        .filter(|entry| entry.is_folder || entry.is_supported_audio())
                        .filter(|entry| {
                            let in_scope = match browser.search_scope {
                                crate::state::BrowserSearchScope::SelectedFolder => {
                                    in_current_tree(entry)
                                }
                                crate::state::BrowserSearchScope::Root
                                | crate::state::BrowserSearchScope::Everywhere => true,
                            };
                            in_scope
                                && (entry.name.to_ascii_lowercase().contains(&query)
                                    || entry.path.to_ascii_lowercase().contains(&query))
                        })
                        .cloned()
                        .collect()
                } else {
                    browser
                        .remote
                        .catalog_child_indices(current)
                        .iter()
                        .filter_map(|&index| browser.remote.catalog.entries.get(index))
                        .filter(|entry| entry.is_folder || entry.is_supported_audio())
                        .cloned()
                        .collect()
                };
                if searching {
                    remote.sort_by(|left, right| {
                        (!left.is_folder, left.name.to_ascii_lowercase())
                            .cmp(&(!right.is_folder, right.name.to_ascii_lowercase()))
                    });
                }
                let remote_visible = remote.len().min(browser.results_visible_limit);
                let mut combined: Vec<_> = remote
                    .into_iter()
                    .take(remote_visible)
                    .enumerate()
                    .filter(|(_, entry)| !entry.is_folder)
                    .map(|(row_index, entry)| BrowserNavigableResult {
                        source: MediaSourceRef::DropboxFile {
                            path_lower: entry.provider_item_id.clone(),
                            display_path: entry.path.clone(),
                            rev: entry.revision.clone(),
                        },
                        remote: Some(entry),
                        row_index,
                        visible_rows: 0,
                    })
                    .collect();
                let mut local_visible = 0;
                if searching && browser.search_scope == crate::state::BrowserSearchScope::Everywhere
                {
                    let mut local: Vec<_> = browser
                        .entries
                        .iter()
                        .filter(|entry| {
                            entry.name.to_ascii_lowercase().contains(&query)
                                || entry
                                    .relative_path
                                    .to_string_lossy()
                                    .to_ascii_lowercase()
                                    .contains(&query)
                        })
                        .collect();
                    local.sort_by_key(|entry| entry.name.to_ascii_lowercase());
                    let local: Vec<_> = local
                        .into_iter()
                        .take(browser.results_visible_limit - remote_visible)
                        .collect();
                    local_visible = local.len();
                    combined.extend(local.into_iter().enumerate().map(|(index, entry)| {
                        BrowserNavigableResult {
                            source: entry.source.clone(),
                            remote: None,
                            row_index: remote_visible + index,
                            visible_rows: 0,
                        }
                    }));
                }
                let visible_rows = remote_visible + local_visible;
                combined
                    .iter_mut()
                    .for_each(|entry| entry.visible_rows = visible_rows);
                combined
            }
        };
        if results.is_empty() {
            self.state.status_text = "No media result to select".into();
            return Task::none();
        }
        let current = browser
            .selected_source
            .as_ref()
            .and_then(|selected| results.iter().position(|entry| &entry.source == selected));
        let index = adjacent_result_index(results.len(), current, direction).unwrap();
        let result = results.into_iter().nth(index).unwrap();
        let selection = match result.remote {
            Some(entry) => self.update(Message::ClickRemoteBrowserEntry(entry)),
            None => self.update(Message::ClickLocalBrowserEntry(result.source)),
        };
        let scroll = scrollable::snap_to(
            browser_results_scroll_id(mode),
            scrollable::RelativeOffset {
                x: 0.0,
                y: browser_results_scroll_offset(result.row_index, result.visible_rows),
            },
        );
        Task::batch([selection, scroll])
    }

    pub(super) fn stop_browser_audition(&mut self) {
        self.send_command(EngineCommand::StopAudition);
        self.state.browser.cancel_audition_requests();
    }

    pub(super) fn start_browser_audition(&mut self, audio: Arc<DecodedAudio>) {
        let queued =
            self.state.transport.playing && self.state.browser.audition_sync != AuditionSync::Off;
        // Retain a UI-side clone (never cleared on stop) so the engine
        // voice can never drop the final Arc inside the RT callback.
        self.state.browser.audition_audio = Some(Arc::clone(&audio));
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
        let generation = self.state.browser.begin_audition_load(&source);
        self.state.status_text = format!("Preparing WARP at {source_bpm:.1} BPM...");
        Task::perform(
            warp_browser_audition_async(raw, source_bpm, project_bpm),
            move |result| Message::BrowserAuditionWarpReady {
                source: source.clone(),
                generation,
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

    pub(super) fn prepare_browser_sample_import(
        &mut self,
        target: BrowserImportTarget,
        treatment: crate::state::AuditionImportInput,
        raw: Arc<vibez_core::audio_buffer::DecodedAudio>,
        name: String,
        source: MediaSourceRef,
    ) -> Task<Message> {
        let project_bpm = self.state.transport.bpm;
        self.state.status_text = match treatment.mode {
            AuditionMode::Raw => format!("Preparing RAW import: {name}"),
            AuditionMode::Warp => format!("Preparing WARP import: {name}"),
        };
        let retained_target = target.clone();
        let generation = self.browser_import_generation;
        Task::perform(
            prepare_browser_import_audio_async(target, treatment, raw, source, project_bpm),
            move |result| match result {
                Ok((audio, original_audio, source)) => Message::BrowserImportPrepared {
                    target: retained_target.clone(),
                    generation,
                    payload: PreparedBrowserImport {
                        treatment,
                        audio,
                        original_audio,
                        name: name.clone(),
                        source,
                    },
                },
                Err(error) => Message::BrowserSampleDecodeError(error),
            },
        )
    }

    pub(super) fn apply_browser_import_prepared(
        &mut self,
        target: BrowserImportTarget,
        payload: PreparedBrowserImport,
    ) -> Task<Message> {
        match target {
            BrowserImportTarget::ArrangementClip(preferred_track) => {
                let track_id = self.ensure_audio_track_for_import(preferred_track);
                let position = self.state.transport.position_samples;
                self.add_audio_clip_to_track_at(track_id, position, payload)
            }
            BrowserImportTarget::ArrangementClipAt {
                track_id,
                position_samples,
            } => self.add_audio_clip_to_track_at(track_id, position_samples, payload),
            BrowserImportTarget::ArrangementNewTrackAt { position_samples } => {
                let track_id = self.ensure_audio_track_for_import(None);
                self.add_audio_clip_to_track_at(track_id, position_samples, payload)
            }
            BrowserImportTarget::Sampler(track_id) => {
                let PreparedBrowserImport {
                    treatment,
                    audio,
                    name,
                    source,
                    ..
                } = payload;
                let provenance = source.provenance().map(|value| value.display_label());
                self.apply_sampler_sample_loaded(track_id, audio, name.clone(), source);
                self.state.status_text = format!(
                    "Imported '{name}' to Sampler ({}){}",
                    treatment.mode.label(),
                    provenance
                        .map(|value| format!(" · source {value}"))
                        .unwrap_or_default()
                );
                Task::none()
            }
            BrowserImportTarget::DrumRackPad {
                track_id,
                pad_index,
            } => {
                let PreparedBrowserImport {
                    treatment,
                    audio,
                    name,
                    source,
                    ..
                } = payload;
                let provenance = source.provenance().map(|value| value.display_label());
                self.apply_drum_rack_pad_loaded(track_id, pad_index, audio, name.clone(), source);
                self.state.status_text = format!(
                    "Imported '{name}' to Drum Rack pad {} ({}){}",
                    pad_index + 1,
                    treatment.mode.label(),
                    provenance
                        .map(|value| format!(" · source {value}"))
                        .unwrap_or_default()
                );
                Task::none()
            }
        }
    }

    pub(super) fn add_audio_clip_to_track_at(
        &mut self,
        track_id: TrackId,
        position_samples: u64,
        payload: PreparedBrowserImport,
    ) -> Task<Message> {
        let PreparedBrowserImport {
            treatment,
            audio,
            original_audio,
            name,
            source,
        } = payload;
        let provenance = source.provenance().map(|value| value.display_label());
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
        let (original_bpm, warped, warped_to_bpm) = match treatment.mode {
            AuditionMode::Raw => (None, false, None),
            AuditionMode::Warp => (treatment.source_bpm, true, Some(self.state.transport.bpm)),
        };

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
                original_bpm,
                warped,
                warped_to_bpm,
                original_audio,
            });
        }
        self.state.arrangement.selected_track = Some(track_id);
        let beat = if self.state.transport.sample_rate > 0 && self.state.transport.bpm > 0.0 {
            position_samples as f64 * self.state.transport.bpm
                / (self.state.transport.sample_rate as f64 * 60.0)
        } else {
            0.0
        };
        self.state.status_text = format!(
            "Imported '{name}' to {track_name} at beat {beat:.2} ({}){}",
            treatment.mode.label(),
            provenance
                .map(|value| format!(" · source {value}"))
                .unwrap_or_default()
        );
        Task::none()
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
        let Some(treatment) = self.state.browser.audition_import_input() else {
            self.state.status_text =
                "Confirm a positive source BPM before importing in WARP mode".into();
            return Task::none();
        };
        if treatment.mode == AuditionMode::Warp
            && self.state.browser.selected_source.as_ref() != Some(&source)
        {
            self.state.status_text =
                "Select this source and confirm its BPM before WARP import".into();
            return Task::none();
        }
        match source {
            MediaSourceRef::LocalFile { path } => {
                let name = path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                self.state.status_text = format!("Dropping {name}...");
                Task::perform(
                    decode_and_stage_local_async(path),
                    move |result| match result {
                        Ok((audio, source)) => Message::BrowserSampleDecoded(
                            target.clone(),
                            treatment,
                            Arc::new(audio),
                            name.clone(),
                            source,
                        ),
                        Err(err) => Message::BrowserSampleDecodeError(err),
                    },
                )
            }
            MediaSourceRef::StagedProjectMedia {
                id,
                file_name,
                staging_path,
                source_path,
            } => {
                let name = file_name.clone();
                let retained_source = MediaSourceRef::StagedProjectMedia {
                    id,
                    file_name,
                    staging_path: staging_path.clone(),
                    source_path,
                };
                Task::perform(
                    decode_file_async(staging_path),
                    move |result| match result {
                        Ok(audio) => Message::BrowserSampleDecoded(
                            target.clone(),
                            treatment,
                            Arc::new(audio),
                            name.clone(),
                            retained_source.clone(),
                        ),
                        Err(err) => Message::BrowserSampleDecodeError(err),
                    },
                )
            }
            MediaSourceRef::StagedRemoteProjectMedia {
                id,
                file_name,
                staging_path,
                provenance,
            } => {
                let name = file_name.clone();
                let retained_source = MediaSourceRef::StagedRemoteProjectMedia {
                    id,
                    file_name,
                    staging_path: staging_path.clone(),
                    provenance,
                };
                Task::perform(
                    decode_file_async(staging_path),
                    move |result| match result {
                        Ok(audio) => Message::BrowserSampleDecoded(
                            target.clone(),
                            treatment,
                            Arc::new(audio),
                            name.clone(),
                            retained_source.clone(),
                        ),
                        Err(err) => Message::BrowserSampleDecodeError(err),
                    },
                )
            }
            MediaSourceRef::ProjectMedia {
                id,
                file_name,
                provenance,
            } => {
                let name = file_name.clone();
                let retained_source = MediaSourceRef::ProjectMedia {
                    id,
                    file_name,
                    provenance,
                };
                let container_path = self.state.project.current_path.clone();
                Task::perform(
                    async move {
                        hydrate_saved_source(container_path.as_ref(), None, &retained_source, &name)
                            .await
                            .map(|audio| (audio, retained_source, name))
                    },
                    move |result| match result {
                        Ok((audio, source, name)) => Message::BrowserSampleDecoded(
                            target.clone(),
                            treatment,
                            Arc::new(audio),
                            name,
                            source,
                        ),
                        Err(err) => Message::BrowserSampleDecodeError(err),
                    },
                )
            }
            MediaSourceRef::DropboxFile {
                path_lower,
                display_path,
                rev,
            } => {
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
                self.start_remote_import(entry, target, treatment)
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
        let selected = self.state.browser.remote.selected_path.as_ref()?;
        self.state
            .browser
            .remote
            .catalog
            .entries
            .iter()
            .find(|entry| &entry.provider_item_id == selected && !entry.is_folder)
            .map(|entry| DropboxEntry {
                path_lower: entry.provider_item_id.clone(),
                path_display: entry.path.clone(),
                name: entry.name.clone(),
                is_folder: false,
                rev: entry.revision.clone(),
                size: entry.size,
            })
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
                    .add_filter(
                        "Supported Audio",
                        vibez_core::audio_format::SUPPORTED_AUDIO_EXTENSIONS,
                    )
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
            return Task::perform(
                decode_and_stage_local_async(path),
                move |result| match result {
                    Ok((audio, source)) => Message::ClipAudioDecoded(
                        track_id,
                        clip_id,
                        Arc::new(audio),
                        file_name.clone(),
                        source,
                    ),
                    Err(e) => Message::ClipDecodeError(track_id, e),
                },
            );
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

        self.state.status_text = format!("Added clip: {name}");
        self.schedule_auto_warp_if_enabled(track_id, clip_id, audio)
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
            return Task::perform(
                decode_and_stage_local_async(path),
                move |result| match result {
                    Ok((audio, source)) => Message::DrumRackPadSampleDecoded(
                        track_id,
                        pad_index,
                        Arc::new(audio),
                        file_name.clone(),
                        source,
                    ),
                    Err(e) => Message::DrumRackPadDecodeError(track_id, pad_index, e),
                },
            );
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
                self.state.browser.cancel_media_drag();
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
        if let Some(source) = self.state.browser.selected_source.clone() {
            let name = source.display_name();
            let position_samples = self.state.transport.position_samples;
            let selected_audio = self.state.arrangement.selected_track.filter(|track_id| {
                self.state
                    .find_track(*track_id)
                    .is_some_and(|track| matches!(track.kind, TrackKind::Audio))
            });
            let target = match selected_audio {
                Some(track_id) => BrowserImportTarget::ArrangementClipAt {
                    track_id,
                    position_samples,
                },
                None => BrowserImportTarget::ArrangementNewTrackAt { position_samples },
            };
            self.state.status_text = format!("Importing {name} at playhead...");
            return self.dispatch_drop_for_target(source, target);
        }
        Task::none()
    }

    pub(super) fn handle_load_selected_browser_sample_to_device(&mut self) -> Task<Message> {
        let Some(source) = self.state.browser.selected_source.clone() else {
            return Task::none();
        };
        let Some(target) = self.selected_browser_device_target() else {
            self.state.status_text =
                "Select a sampler or drum rack track to load from the browser".to_string();
            return Task::none();
        };
        self.state.status_text = format!("Loading {}...", source.display_name());
        self.dispatch_drop_for_target(source, target)
    }
}

#[cfg(test)]
mod browser_keyboard_navigation_tests {
    use super::{adjacent_result_index, browser_results_scroll_offset};

    #[test]
    fn adjacent_result_navigation_selects_edges_and_clamps() {
        assert_eq!(adjacent_result_index(0, None, 1), None);
        assert_eq!(adjacent_result_index(3, None, 1), Some(0));
        assert_eq!(adjacent_result_index(3, None, -1), Some(2));
        assert_eq!(adjacent_result_index(3, Some(1), 1), Some(2));
        assert_eq!(adjacent_result_index(3, Some(2), 1), Some(2));
        assert_eq!(adjacent_result_index(3, Some(0), -1), Some(0));
    }

    #[test]
    fn keyboard_selection_maps_to_the_results_scroll_range() {
        assert_eq!(browser_results_scroll_offset(0, 1), 0.0);
        assert_eq!(browser_results_scroll_offset(0, 100), 0.0);
        assert!((browser_results_scroll_offset(50, 100) - 0.505).abs() < 0.001);
        assert_eq!(browser_results_scroll_offset(99, 100), 1.0);
    }
}
