//! Background audio tasks: quantize renders, library scanning,
//! MIDI auto-open.

//! Split out of app.rs; inherent methods on [`super::App`].

use std::path::PathBuf;
use std::sync::Arc;

use vibez_core::id::ClipId;
use vibez_core::track::MediaSourceRef;

use crate::message::SampleLibraryScanResult;
use crate::state::{SampleBrowserEntry, SampleBrowserFolder};

pub(crate) struct AutoWarpInput {
    pub(crate) audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
    pub(crate) sample_rate: u32,
    pub(crate) project_bpm: f64,
    pub(crate) confidence_threshold: f32,
}

pub(crate) struct QuantizeInput {
    pub(crate) audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
    pub(crate) bpm: f64,
    pub(crate) sample_rate: u32,
    pub(crate) grid: crate::state::SnapGrid,
    pub(crate) clip_position: u64,
    pub(crate) clip_source_offset: u64,
    pub(crate) clip_duration: u64,
    pub(crate) original_name: String,
    pub(crate) new_clip_id: ClipId,
}

pub(crate) fn auto_open_midi_input(
    preferred: Option<&str>,
) -> Option<vibez_audio_io::midi_input::MidiInputHandle> {
    let ports = vibez_audio_io::midi_input::list_midi_input_ports().ok()?;
    let target = preferred
        .and_then(|name| ports.iter().find(|p| p.as_str() == name).cloned())
        .or_else(|| ports.into_iter().next())?;
    vibez_audio_io::midi_input::open_midi_input(&target).ok()
}

pub(crate) fn compute_audio_quantize(
    input: QuantizeInput,
) -> Result<crate::message::AudioQuantizeSuccess, String> {
    const DEFAULT_SENSITIVITY: f32 = 1.5;
    const MIN_SLICE_FRAMES: usize = 64;

    let QuantizeInput {
        audio,
        bpm,
        sample_rate,
        grid,
        clip_position,
        clip_source_offset,
        clip_duration,
        original_name,
        new_clip_id,
    } = input;

    let sr = sample_rate as f64;
    let clip_end_src = clip_source_offset.saturating_add(clip_duration);
    let onsets: Vec<u64> = vibez_core::onset::detect_onsets(&audio, DEFAULT_SENSITIVITY)
        .into_iter()
        .filter(|&o| o >= clip_source_offset && o < clip_end_src)
        .collect();
    if onsets.is_empty() {
        return Err("No transients detected in clip".into());
    }

    let samples_to_beats = |s: u64| -> f64 { s as f64 * bpm / (sr * 60.0) };
    let beats_to_samples = |beats: f64| -> u64 {
        if bpm > 0.0 && sr > 0.0 {
            (beats * sr * 60.0 / bpm) as u64
        } else {
            0
        }
    };

    let mut source_bounds: Vec<u64> = onsets.clone();
    source_bounds.push(clip_end_src);

    let mut target_positions: Vec<u64> = Vec::with_capacity(source_bounds.len());
    for &b in &source_bounds {
        let original_timeline_pos = clip_position.saturating_add(b - clip_source_offset);
        let snapped_beats = grid
            .snap_beat(samples_to_beats(original_timeline_pos))
            .max(0.0);
        target_positions.push(beats_to_samples(snapped_beats));
    }
    for i in 1..target_positions.len() {
        if target_positions[i] < target_positions[i - 1] {
            target_positions[i] = target_positions[i - 1];
        }
    }

    let channel_count = audio.channels.len().max(1);
    let mut output_channels: Vec<Vec<f32>> = (0..channel_count).map(|_| Vec::new()).collect();
    let mut total_out_len: usize = 0;
    let mut stretched_slices = 0usize;

    for i in 0..onsets.len() {
        let src_start = source_bounds[i] as usize;
        let src_end = source_bounds[i + 1] as usize;
        let src_len = src_end.saturating_sub(src_start);
        let target_len = target_positions[i + 1].saturating_sub(target_positions[i]) as usize;
        if src_len < MIN_SLICE_FRAMES || target_len == 0 {
            continue;
        }
        // Build a per-slice DecodedAudio so pitch_preserving_stretch
        // can run a single shared analysis pass across channels.
        // Extreme ratios (very short/long snap targets) route through
        // the linear resampler automatically; typical ratios go
        // through WSOLA and preserve pitch on the bass-loop case the
        // user reported.
        let slice_channels: Vec<Vec<f32>> = audio
            .channels
            .iter()
            .map(|c| {
                let start = src_start.min(c.len());
                let end = src_end.min(c.len());
                c[start..end].to_vec()
            })
            .collect();
        let slice_audio = vibez_core::audio_buffer::DecodedAudio {
            channels: slice_channels,
            sample_rate,
        };
        let stretched_slice =
            vibez_dsp::time_stretch::pitch_preserving_stretch(&slice_audio, target_len);
        for (ch, out) in output_channels.iter_mut().enumerate() {
            if let Some(s) = stretched_slice.channels.get(ch) {
                out.extend_from_slice(s);
            }
        }
        total_out_len += target_len;
        stretched_slices += 1;
    }

    if stretched_slices == 0 || total_out_len == 0 {
        return Err("All slices collapsed to zero length after snapping".into());
    }

    for ch in output_channels.iter_mut() {
        if ch.len() < total_out_len {
            ch.resize(total_out_len, 0.0);
        } else {
            ch.truncate(total_out_len);
        }
    }

    let new_audio = Arc::new(vibez_core::audio_buffer::DecodedAudio {
        channels: output_channels,
        sample_rate,
    });

    Ok(crate::message::AudioQuantizeSuccess {
        new_clip_id,
        new_audio,
        new_name: format!("{original_name} (Q {})", grid.label()),
        new_position: target_positions[0],
        new_duration: total_out_len as u64,
        slice_count: stretched_slices,
        grid_label: grid.label().to_string(),
    })
}

pub(crate) fn scan_root_into(
    root: &PathBuf,
    dir: &PathBuf,
    entries: &mut Vec<SampleBrowserEntry>,
    folders: &mut Vec<SampleBrowserFolder>,
    warnings: &mut Vec<String>,
) {
    let read_dir = match std::fs::read_dir(dir) {
        Ok(read_dir) => read_dir,
        Err(err) => {
            warnings.push(format!("Failed to read {} ({err})", dir.display()));
            return;
        }
    };

    for item in read_dir {
        let item = match item {
            Ok(item) => item,
            Err(err) => {
                warnings.push(format!(
                    "Failed to read an item in {} ({err})",
                    dir.display()
                ));
                continue;
            }
        };
        let path = item.path();
        if path
            .strip_prefix(root)
            .ok()
            .is_some_and(path_contains_hidden_component)
        {
            continue;
        }
        let file_type = match item.file_type() {
            Ok(file_type) => file_type,
            Err(err) => {
                warnings.push(format!("Failed to inspect {} ({err})", path.display()));
                continue;
            }
        };
        if file_type.is_symlink() {
            warnings.push(format!("Skipped symbolic link: {}", path.display()));
            continue;
        }
        if file_type.is_dir() {
            let relative_path = path
                .strip_prefix(root)
                .map(|relative| relative.to_path_buf())
                .unwrap_or_else(|_| path.clone());
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            folders.push(SampleBrowserFolder {
                path: path.clone(),
                root_path: root.clone(),
                search_text: format!(
                    "{} {} {}",
                    name.to_lowercase(),
                    relative_path.display().to_string().to_lowercase(),
                    root.display().to_string().to_lowercase()
                ),
                relative_path,
                name,
            });
            scan_root_into(root, &path, entries, folders, warnings);
            continue;
        }
        if !file_type.is_file() || !is_supported_audio_file(&path) {
            continue;
        }
        let relative_path = path
            .strip_prefix(root)
            .map(|rel| rel.to_path_buf())
            .unwrap_or_else(|_| path.clone());
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        let search_text = format!(
            "{} {} {} {}",
            name.to_lowercase(),
            relative_path.display().to_string().to_lowercase(),
            root.display().to_string().to_lowercase(),
            path.extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or_default()
                .to_lowercase()
        );
        let metadata = item.metadata().ok();
        entries.push(SampleBrowserEntry {
            source: MediaSourceRef::LocalFile { path },
            name,
            root_path: root.clone(),
            relative_path,
            format: vibez_core::audio_format::audio_format_for_path(&item.path())
                .map(|format| format.label)
                .unwrap_or("AUDIO")
                .to_string(),
            duration_seconds: None,
            channels: None,
            sample_rate: None,
            file_size: metadata.as_ref().map(std::fs::Metadata::len),
            modified: metadata.and_then(|metadata| metadata.modified().ok()),
            search_text,
        });
    }
}

pub(crate) fn scan_sample_root(root: &PathBuf) -> Result<SampleLibraryScanResult, String> {
    if !root.is_dir() {
        return Err(format!("Root is unavailable: {}", root.display()));
    }
    let mut entries = Vec::new();
    let mut folders = Vec::new();
    let mut warnings = Vec::new();
    scan_root_into(root, root, &mut entries, &mut folders, &mut warnings);
    entries.sort_by(|a, b| {
        a.relative_path
            .cmp(&b.relative_path)
            .then_with(|| a.name.cmp(&b.name))
    });
    folders.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(SampleLibraryScanResult {
        entries,
        folders,
        warnings,
    })
}

pub(crate) fn path_contains_hidden_component(path: &std::path::Path) -> bool {
    path.components().any(|component| {
        let std::path::Component::Normal(name) = component else {
            return false;
        };
        name.to_str()
            .is_some_and(|name| name.starts_with('.') && name != "." && name != "..")
    })
}

pub(crate) fn is_supported_audio_file(path: &std::path::Path) -> bool {
    vibez_core::audio_format::is_supported_audio_path(path)
}

#[cfg(test)]
mod local_catalog_tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, Instant};

    fn snapshot_tree(root: &std::path::Path) -> Vec<(PathBuf, Option<Vec<u8>>)> {
        fn visit(
            root: &std::path::Path,
            directory: &std::path::Path,
            snapshot: &mut Vec<(PathBuf, Option<Vec<u8>>)>,
        ) {
            let mut children: Vec<_> = fs::read_dir(directory)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect();
            children.sort();
            for path in children {
                let relative = path.strip_prefix(root).unwrap().to_path_buf();
                if path.is_dir() {
                    snapshot.push((relative, None));
                    visit(root, &path, snapshot);
                } else {
                    snapshot.push((relative, Some(fs::read(path).unwrap())));
                }
            }
        }

        let mut snapshot = Vec::new();
        visit(root, root, &mut snapshot);
        snapshot
    }

    #[test]
    fn local_catalog_is_read_only_media_only_and_preserves_source_identity() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("Samples");
        let drums = root.join("Drums");
        let nested = drums.join("One Shots");
        fs::create_dir_all(&nested).unwrap();
        fs::write(drums.join("kick.wav"), b"same bytes").unwrap();
        fs::write(nested.join("kick-copy.wav"), b"same bytes").unwrap();
        fs::write(drums.join("notes.pdf"), b"not media").unwrap();
        fs::write(root.join("cover.png"), b"not media").unwrap();
        let before = snapshot_tree(&root);

        let mut entries = Vec::new();
        let mut folders = Vec::new();
        let mut warnings = Vec::new();
        scan_root_into(&root, &root, &mut entries, &mut folders, &mut warnings);

        assert_eq!(snapshot_tree(&root), before);
        assert!(warnings.is_empty());
        assert_eq!(entries.len(), 2);
        assert_eq!(folders.len(), 2);
        assert!(entries.iter().all(|entry| entry.format == "WAV"));
        assert!(entries.iter().all(|entry| entry.file_size == Some(10)));
        assert_ne!(entries[0].source, entries[1].source);
        assert!(entries
            .iter()
            .all(|entry| !entry.name.ends_with(".pdf") && !entry.name.ends_with(".png")));
    }

    #[test]
    fn local_catalog_uses_the_shared_audio_support_matrix() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("Formats");
        fs::create_dir_all(&root).unwrap();
        for extension in vibez_core::audio_format::SUPPORTED_AUDIO_EXTENSIONS {
            fs::write(root.join(format!("fixture.{extension}")), []).unwrap();
        }
        fs::write(root.join("upper.M4A"), []).unwrap();
        fs::write(root.join("raw.aac"), []).unwrap();
        fs::write(root.join("notes.mid"), []).unwrap();

        let catalog = scan_sample_root(&root).unwrap();

        assert_eq!(
            catalog.entries.len(),
            vibez_core::audio_format::SUPPORTED_AUDIO_EXTENSIONS.len() + 1
        );
        assert!(catalog
            .entries
            .iter()
            .any(|entry| entry.name == "upper.M4A"));
        assert!(!catalog.entries.iter().any(|entry| entry.name == "raw.aac"));
        assert!(!catalog
            .entries
            .iter()
            .any(|entry| entry.name == "notes.mid"));
    }

    #[test]
    fn large_synthetic_catalog_scan_remains_bounded_in_wall_time() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("Large Samples");
        fs::create_dir_all(&root).unwrap();
        for index in 0..2_000 {
            fs::write(root.join(format!("sample-{index:04}.wav")), []).unwrap();
        }

        let mut entries = Vec::new();
        let mut folders = Vec::new();
        let mut warnings = Vec::new();
        let started = Instant::now();
        scan_root_into(&root, &root, &mut entries, &mut folders, &mut warnings);

        assert_eq!(entries.len(), 2_000);
        assert!(warnings.is_empty());
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "2,000-entry scan took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn local_catalog_ignores_hidden_entries() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("Samples");
        fs::create_dir_all(root.join(".cache")).unwrap();
        fs::create_dir_all(root.join("Visible")).unwrap();
        fs::write(root.join(".hidden.wav"), []).unwrap();
        fs::write(root.join(".cache/inside.wav"), []).unwrap();
        fs::write(root.join("Visible/keep.wav"), []).unwrap();

        let catalog = scan_sample_root(&root).unwrap();

        assert_eq!(catalog.entries.len(), 1);
        assert_eq!(catalog.entries[0].name, "keep.wav");
        assert_eq!(catalog.folders.len(), 1);
        assert_eq!(catalog.folders[0].name, "Visible");
    }

    #[cfg(unix)]
    #[test]
    fn local_catalog_does_not_follow_symlinks_outside_the_root() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("Samples");
        let outside = temporary.path().join("Outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("escape.wav"), []).unwrap();
        symlink(&outside, root.join("linked-outside")).unwrap();

        let catalog = scan_sample_root(&root).unwrap();

        assert!(catalog.entries.is_empty());
        assert!(catalog.folders.is_empty());
        assert_eq!(catalog.warnings.len(), 1);
        assert!(catalog.warnings[0].contains("Skipped symbolic link"));
    }
}
