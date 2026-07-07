//! Background audio tasks: quantize renders, library scanning,
//! MIDI auto-open.

//! Split out of app.rs; inherent methods on [`super::App`].

use std::path::PathBuf;
use std::sync::Arc;

use vibez_core::id::ClipId;
use vibez_core::track::MediaSourceRef;

use crate::state::SampleBrowserEntry;

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
        let Ok(item) = item else {
            continue;
        };
        let path = item.path();
        if path.is_dir() {
            scan_root_into(root, &path, entries, warnings);
            continue;
        }
        if !is_supported_audio_file(&path) {
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
            "{} {} {}",
            name.to_lowercase(),
            relative_path.display().to_string().to_lowercase(),
            root.display().to_string().to_lowercase()
        );
        entries.push(SampleBrowserEntry {
            source: MediaSourceRef::LocalFile { path },
            name,
            root_path: root.clone(),
            relative_path,
            search_text,
        });
    }
}

pub(crate) fn is_supported_audio_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "wav" | "wave" | "mp3" | "flac" | "ogg" | "aac" | "m4a" | "aif" | "aiff"
            )
        })
        .unwrap_or(false)
}
