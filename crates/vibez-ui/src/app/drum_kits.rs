//! Loading Drum Rack kits onto a rack, the factory ones built in or a kit
//! folder, and saving a rack as a kit folder of its own.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use vibez_core::drum_kit::{DrumKit, KIT_FILE};
use vibez_core::id::TrackId;

use crate::domains::drum_kits::{kit_from_rack, rack_pads, KitSample, SampleToWrite, FACTORY_KITS};

use super::*;

/// One of a kit's samples decoded and staged as project media, from its
/// bytes and where they came from.
fn stage_sample(file: &str, origin: &Path, bytes: &[u8]) -> Result<KitSample, String> {
    let extension = Path::new(file)
        .extension()
        .map(|value| value.to_string_lossy().into_owned());
    let audio = vibez_audio_io::file_io::decode_audio_cursor(
        std::io::Cursor::new(bytes.to_vec()),
        extension.as_deref(),
    )
    .map_err(|error| format!("{file}: {error}"))?;
    let source = vibez_project::project_format_v1::stage_local_content(origin, file, bytes)
        .map_err(|error| format!("{file}: {error}"))?;
    Ok(KitSample {
        file: file.to_string(),
        audio: Arc::new(audio),
        source,
    })
}

/// Each sample the kit names once, in the order it names them.
fn unique_samples(kit: &DrumKit) -> Vec<String> {
    let mut files: Vec<String> = Vec::new();
    for pad in &kit.pads {
        if !files.contains(&pad.sample) {
            files.push(pad.sample.clone());
        }
    }
    files
}

fn read_kit_folder(folder: &Path) -> Result<(DrumKit, Vec<KitSample>), String> {
    let text = std::fs::read_to_string(folder.join(KIT_FILE))
        .map_err(|error| format!("{KIT_FILE}: {error}"))?;
    let kit: DrumKit = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    kit.validate().map_err(|error| error.to_string())?;
    let mut samples = Vec::new();
    for file in unique_samples(&kit) {
        let path = folder.join(&file);
        // A missing sample leaves its pad silent rather than the kit unloaded.
        if let Ok(bytes) = std::fs::read(&path) {
            samples.push(stage_sample(&file, &path, &bytes)?);
        }
    }
    Ok((kit, samples))
}

fn write_kit_folder(folder: &Path, kit: &DrumKit, samples: &[SampleToWrite]) -> Result<(), String> {
    std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    for (file, audio) in samples {
        std::fs::write(
            folder.join(file),
            vibez_audio_io::file_io::encode_wav(audio),
        )
        .map_err(|error| format!("{file}: {error}"))?;
    }
    let text = serde_json::to_string_pretty(kit).map_err(|error| error.to_string())?;
    std::fs::write(folder.join(KIT_FILE), text).map_err(|error| error.to_string())
}

impl App {
    pub(super) fn load_factory_drum_kit(
        &mut self,
        track_id: TrackId,
        index: usize,
    ) -> Task<Message> {
        let Some(factory) = FACTORY_KITS.get(index) else {
            return Task::none();
        };
        self.state.status_text = format!("Loading kit {}...", factory.name);
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    let kit = factory.kit();
                    let mut samples = Vec::new();
                    for file in unique_samples(&kit) {
                        let bytes = factory.sample(&file).ok_or_else(|| file.clone())?;
                        let origin = PathBuf::from("factory-kits")
                            .join(factory.folder)
                            .join(&file);
                        samples.push(stage_sample(&file, &origin, bytes)?);
                    }
                    Ok((kit, samples))
                })
                .await
                .map_err(|error| error.to_string())?
            },
            move |result| Message::DrumKitReady(track_id, result),
        )
    }

    pub(super) fn open_drum_kit(&mut self, track_id: TrackId) -> Task<Message> {
        Task::perform(
            async {
                rfd::AsyncFileDialog::new()
                    .set_title("Open Drum Kit Folder")
                    .pick_folder()
                    .await
                    .map(|folder| folder.path().to_path_buf())
            },
            move |folder| Message::DrumKitFolderSelected(track_id, folder),
        )
    }

    pub(super) fn read_drum_kit(
        &mut self,
        track_id: TrackId,
        folder: Option<PathBuf>,
    ) -> Task<Message> {
        let Some(folder) = folder else {
            return Task::none();
        };
        self.state.status_text = "Loading kit...".into();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || read_kit_folder(&folder))
                    .await
                    .map_err(|error| error.to_string())?
            },
            move |result| Message::DrumKitReady(track_id, result),
        )
    }

    pub(super) fn apply_drum_kit(
        &mut self,
        track_id: TrackId,
        result: Result<(DrumKit, Vec<KitSample>), String>,
    ) -> Task<Message> {
        let (kit, samples) = match result {
            Ok(loaded) => loaded,
            Err(error) => {
                self.state.status_text = format!("Kit not loaded: {error}");
                return Task::none();
            }
        };
        let missing = unique_samples(&kit).len() - samples.len();
        self.instrument_panels.entry(track_id).or_default().kit = Some(kit.name.clone());
        let task = self.update(Message::Devices(
            crate::domains::devices::DevicesMsg::ReplaceDrumRackPads(
                track_id,
                rack_pads(&kit, &samples),
            ),
        ));
        // Shows the kit rather than a bank it may leave empty.
        if let (Some(first), Some(track)) = (kit.pads.first(), self.state.find_track_mut(track_id))
        {
            track.selected_drum_pad = first.pad;
        }
        self.state.status_text = if missing == 0 {
            format!("Loaded kit {}", kit.name)
        } else {
            format!(
                "Loaded kit {}; {missing} of its samples are missing",
                kit.name
            )
        };
        task
    }

    pub(super) fn choose_drum_kit_folder(&mut self, track_id: TrackId) -> Task<Message> {
        let name = self
            .instrument_panels
            .get(&track_id)
            .and_then(|panel| panel.kit.clone())
            .unwrap_or_else(|| "My Kit".into());
        Task::perform(
            async move {
                rfd::AsyncFileDialog::new()
                    .set_title("Save Drum Kit As")
                    .set_file_name(name)
                    .save_file()
                    .await
                    .map(|folder| folder.path().to_path_buf())
            },
            move |folder| Message::DrumKitSaveSelected(track_id, folder),
        )
    }

    pub(super) fn save_drum_kit(
        &mut self,
        track_id: TrackId,
        folder: Option<PathBuf>,
    ) -> Task<Message> {
        let Some(folder) = folder else {
            return Task::none();
        };
        let Some(track) = self.state.find_track(track_id) else {
            return Task::none();
        };
        let name = folder
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "My Kit".into());
        let (kit, samples) = match kit_from_rack(&name, &track.drum_rack_pads) {
            Ok(saved) => saved,
            Err(error) => {
                self.state.status_text = format!("Kit not saved: {error}");
                return Task::none();
            }
        };
        self.instrument_panels.entry(track_id).or_default().kit = Some(name.clone());
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    write_kit_folder(&folder, &kit, &samples).map(|()| name)
                })
                .await
                .map_err(|error| error.to_string())?
            },
            Message::DrumKitSaved,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::drum_kits::KitSample;
    use vibez_instruments::sampler::param;

    #[test]
    fn a_saved_kit_folder_reads_back_with_its_samples() {
        let folder = tempfile::tempdir().unwrap();
        let mut pads = crate::state::default_drum_rack_pads();
        pads[3].name = Some("Clap".into());
        pads[3].audio = Some(Arc::new(vibez_core::audio_buffer::DecodedAudio {
            channels: vec![vec![0.25; 64]],
            sample_rate: 44_100,
        }));
        pads[3].sampler_params_mut()[param::TUNE] = -3.0;
        let (kit, samples) = kit_from_rack("Mine", &pads).unwrap();
        write_kit_folder(folder.path(), &kit, &samples).unwrap();

        let (read, staged): (DrumKit, Vec<KitSample>) = read_kit_folder(folder.path()).unwrap();

        assert_eq!(read, kit);
        assert_eq!(staged.len(), 1);
        assert_eq!(staged[0].audio.num_frames(), 64);
        let loaded = rack_pads(&read, &staged);
        assert_eq!(loaded[3].param(param::TUNE), -3.0);
        assert!(loaded[3].audio.is_some());
    }

    #[test]
    fn a_kit_naming_a_missing_sample_still_loads_its_other_pads() {
        let folder = tempfile::tempdir().unwrap();
        let kit = FACTORY_KITS[1].kit();
        std::fs::write(
            folder.path().join(KIT_FILE),
            serde_json::to_string(&kit).unwrap(),
        )
        .unwrap();
        std::fs::write(
            folder.path().join("kick.wav"),
            FACTORY_KITS[1].sample("kick.wav").unwrap(),
        )
        .unwrap();

        let (_, staged) = read_kit_folder(folder.path()).unwrap();

        assert_eq!(staged.len(), 1);
        assert_eq!(staged[0].file, "kick.wav");
    }

    #[test]
    fn loading_a_kit_is_one_undo_step_and_shows_its_first_pad() {
        use crate::domains::devices::DevicesMsg;
        use crate::state::ProjectTrack;
        use vibez_core::midi::{InstrumentKind, TrackKind};
        let mut app = super::super::test_support::app();
        let mut track = ProjectTrack::new(TrackId::new(), "Drums".into(), 0);
        track.kind = TrackKind::Midi;
        let id = track.id;
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(track);
        let _ = app.update(Message::Devices(DevicesMsg::SetTrackInstrument(
            id,
            InstrumentKind::DrumRack,
        )));
        app.state.find_track_mut(id).unwrap().selected_drum_pad = 40;
        app.state.project.history = Default::default();
        let kit = FACTORY_KITS[1].kit();
        let samples = vec![KitSample {
            file: "kick.wav".into(),
            audio: Arc::new(vibez_core::audio_buffer::DecodedAudio {
                channels: vec![vec![0.5; 32]],
                sample_rate: 44_100,
            }),
            source: vibez_core::track::MediaSourceRef::LocalFile {
                path: "/kits/909/kick.wav".into(),
            },
        }];

        let _ = app.apply_drum_kit(id, Ok((kit, samples)));
        let loaded = app.state.find_track(id).unwrap();
        assert_eq!(loaded.drum_rack_pads[0].name.as_deref(), Some("Kick"));
        assert_eq!(loaded.selected_drum_pad, 0);
        assert_eq!(app.instrument_panels[&id].kit.as_deref(), Some("909"));

        let _ = app.update(Message::Project(crate::domains::project::ProjectMsg::Undo));
        assert!(app.state.find_track(id).unwrap().drum_rack_pads[0]
            .name
            .is_none());
    }
}
