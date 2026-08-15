//! Runtime lifecycle and live preview for one Arrange Audio Track take.

use std::sync::Arc;
use std::time::{Duration, Instant};
use vibez_core::id::{ClipId, TrackId};

pub const STOP_ACK_TIMEOUT: Duration = Duration::from_secs(2);
const LIVE_WAVEFORM_FRAMES_PER_PEAK: usize = 64;
const LIVE_WAVEFORM_MAX_PEAKS: usize = 4_096;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AudioRecordingPhase {
    #[default]
    Idle,
    Recording,
    Stopping,
    Finalizing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioRecordingSource {
    HardwareInput,
    TrackOutput(TrackId),
}

#[derive(Debug)]
struct LiveWaveformPreview {
    peaks: Arc<Vec<(f32, f32)>>,
    pending_min: f32,
    pending_max: f32,
    pending_frames: usize,
    frames_per_peak: usize,
}

impl Default for LiveWaveformPreview {
    fn default() -> Self {
        Self {
            peaks: Arc::new(Vec::new()),
            pending_min: 0.0,
            pending_max: 0.0,
            pending_frames: 0,
            frames_per_peak: LIVE_WAVEFORM_FRAMES_PER_PEAK,
        }
    }
}

impl LiveWaveformPreview {
    fn clear(&mut self) {
        self.peaks = Arc::new(Vec::new());
        self.pending_min = 0.0;
        self.pending_max = 0.0;
        self.pending_frames = 0;
        self.frames_per_peak = LIVE_WAVEFORM_FRAMES_PER_PEAK;
    }

    fn extend(&mut self, frames: &[[f32; 2]]) {
        let peaks = Arc::make_mut(&mut self.peaks);
        for frame in frames {
            if self.pending_frames == 0 {
                // Publish the in-progress bucket immediately. Its source span
                // is partial, but the Clip duration bounds drawing to frames
                // that have actually arrived.
                peaks.push((0.0, 0.0));
            }
            self.pending_min = self.pending_min.min(frame[0]).min(frame[1]);
            self.pending_max = self.pending_max.max(frame[0]).max(frame[1]);
            self.pending_frames += 1;
            if let Some(pending) = peaks.last_mut() {
                *pending = (self.pending_min, self.pending_max);
            }
            if self.pending_frames == self.frames_per_peak {
                self.pending_min = 0.0;
                self.pending_max = 0.0;
                self.pending_frames = 0;
                if peaks.len() >= LIVE_WAVEFORM_MAX_PEAKS {
                    for index in 0..peaks.len() / 2 {
                        let left = peaks[index * 2];
                        let right = peaks[index * 2 + 1];
                        peaks[index] = (left.0.min(right.0), left.1.max(right.1));
                    }
                    peaks.truncate(peaks.len() / 2);
                    self.frames_per_peak = self.frames_per_peak.saturating_mul(2);
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct AudioRecordingPreview {
    pub clip_id: ClipId,
    pub position: u64,
    pub duration: u64,
    pub peaks: Arc<Vec<(f32, f32)>>,
    pub frames_per_peak: usize,
    pub source: AudioRecordingSource,
}

#[derive(Debug, Default)]
pub struct AudioRecordingState {
    pub armed_track: Option<TrackId>,
    pub monitor_track: Option<TrackId>,
    pub phase: AudioRecordingPhase,
    pub source: Option<AudioRecordingSource>,
    pub start_position_samples: u64,
    pub captured_frames: Vec<[f32; 2]>,
    preview_clip_id: Option<ClipId>,
    live_waveform: LiveWaveformPreview,
    pub input_peak_l: f32,
    pub input_peak_r: f32,
    pub truncated: bool,
    stop_requested_at: Option<Instant>,
}

impl AudioRecordingState {
    pub fn is_recording(&self) -> bool {
        self.phase == AudioRecordingPhase::Recording
    }
    pub fn is_busy(&self) -> bool {
        self.phase != AudioRecordingPhase::Idle
    }
    pub fn is_capturing(&self) -> bool {
        matches!(
            self.phase,
            AudioRecordingPhase::Recording | AudioRecordingPhase::Stopping
        )
    }

    pub fn arm(&mut self, track_id: TrackId) {
        self.armed_track = Some(track_id);
    }
    pub fn disarm(&mut self) {
        if !self.is_busy() {
            self.armed_track = None;
        }
    }

    pub fn begin(&mut self, position_samples: u64, source: AudioRecordingSource) -> bool {
        if self.armed_track.is_none() || self.phase != AudioRecordingPhase::Idle {
            return false;
        }
        self.captured_frames.clear();
        self.start_position_samples = position_samples;
        self.source = Some(source);
        self.preview_clip_id = Some(ClipId::new());
        self.live_waveform.clear();
        self.truncated = false;
        self.stop_requested_at = None;
        self.phase = AudioRecordingPhase::Recording;
        true
    }

    pub fn captured_frames_appended(&mut self, previous_len: usize) {
        let first_new = previous_len.min(self.captured_frames.len());
        self.live_waveform
            .extend(&self.captured_frames[first_new..]);
    }

    pub fn preview_for_track(&self, track_id: TrackId) -> Option<AudioRecordingPreview> {
        if !self.is_capturing()
            || self.armed_track != Some(track_id)
            || self.captured_frames.is_empty()
        {
            return None;
        }
        Some(AudioRecordingPreview {
            clip_id: self.preview_clip_id?,
            position: self.start_position_samples,
            duration: self.captured_frames.len() as u64,
            peaks: Arc::clone(&self.live_waveform.peaks),
            frames_per_peak: self.live_waveform.frames_per_peak,
            source: self.source?,
        })
    }

    pub fn request_stop(&mut self) -> bool {
        if self.phase != AudioRecordingPhase::Recording {
            return false;
        }
        self.phase = AudioRecordingPhase::Stopping;
        self.stop_requested_at = Some(Instant::now());
        true
    }

    pub fn mark_truncated(&mut self) {
        self.truncated = true;
    }

    pub fn stop_ack_timed_out(&self, now: Instant) -> bool {
        self.phase == AudioRecordingPhase::Stopping
            && self
                .stop_requested_at
                .is_some_and(|requested| now.duration_since(requested) >= STOP_ACK_TIMEOUT)
    }

    pub fn begin_finalizing(&mut self) -> Option<(TrackId, u64, Vec<[f32; 2]>)> {
        if self.phase != AudioRecordingPhase::Stopping {
            return None;
        }
        self.phase = AudioRecordingPhase::Finalizing;
        Some((
            self.armed_track?,
            self.start_position_samples,
            std::mem::take(&mut self.captured_frames),
        ))
    }

    pub fn finish(&mut self) {
        self.phase = AudioRecordingPhase::Idle;
        self.source = None;
        self.preview_clip_id = None;
        self.live_waveform.clear();
        self.stop_requested_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_arm_records_and_finalizes_one_take() {
        let track = TrackId::new();
        let mut state = AudioRecordingState::default();
        state.arm(track);
        assert!(state.begin(4_800, AudioRecordingSource::HardwareInput));
        state.captured_frames.push([0.2, -0.2]);
        assert!(state.request_stop());
        let (target, start, take) = state.begin_finalizing().unwrap();
        assert_eq!((target, start), (track, 4_800));
        assert_eq!(take, vec![[0.2, -0.2]]);
        state.finish();
        assert_eq!(state.phase, AudioRecordingPhase::Idle);
    }

    #[test]
    fn a_missing_output_callback_cannot_leave_stop_pending_forever() {
        let mut state = AudioRecordingState::default();
        state.arm(TrackId::new());
        assert!(state.begin(0, AudioRecordingSource::TrackOutput(TrackId::new())));
        assert!(state.request_stop());
        state.stop_requested_at = Some(Instant::now() - STOP_ACK_TIMEOUT);
        assert!(state.stop_ack_timed_out(Instant::now()));
    }

    #[test]
    fn captured_frames_build_a_live_waveform_preview_across_ui_drains() {
        let track = TrackId::new();
        let mut state = AudioRecordingState::default();
        state.arm(track);
        assert!(state.begin(9_600, AudioRecordingSource::HardwareInput));

        state.captured_frames.extend([[0.25, -0.5]; 40]);
        state.captured_frames_appended(0);
        state.captured_frames.extend([[0.75, -0.2]; 40]);
        state.captured_frames_appended(40);

        let preview = state.preview_for_track(track).unwrap();
        assert_eq!(preview.position, 9_600);
        assert_eq!(preview.duration, 80);
        assert_eq!(preview.frames_per_peak, LIVE_WAVEFORM_FRAMES_PER_PEAK);
        assert_eq!(preview.peaks.as_slice(), &[(-0.5, 0.75), (-0.2, 0.75)]);
        assert!(state.preview_for_track(TrackId::new()).is_none());
    }

    #[test]
    fn long_live_waveforms_compact_without_losing_extrema() {
        let track = TrackId::new();
        let mut state = AudioRecordingState::default();
        state.arm(track);
        assert!(state.begin(0, AudioRecordingSource::HardwareInput));
        state.captured_frames.resize(
            LIVE_WAVEFORM_MAX_PEAKS * LIVE_WAVEFORM_FRAMES_PER_PEAK,
            [0.9, -0.7],
        );
        state.captured_frames_appended(0);

        let preview = state.preview_for_track(track).unwrap();
        assert_eq!(preview.peaks.len(), LIVE_WAVEFORM_MAX_PEAKS / 2);
        assert_eq!(preview.frames_per_peak, LIVE_WAVEFORM_FRAMES_PER_PEAK * 2);
        assert!(preview.peaks.iter().all(|peak| *peak == (-0.7, 0.9)));
    }

    #[test]
    fn partial_peak_stays_visible_after_long_recording_compaction() {
        let track = TrackId::new();
        let mut state = AudioRecordingState::default();
        state.arm(track);
        assert!(state.begin(0, AudioRecordingSource::HardwareInput));
        state.captured_frames.resize(
            LIVE_WAVEFORM_MAX_PEAKS * LIVE_WAVEFORM_FRAMES_PER_PEAK,
            [0.9, -0.7],
        );
        state.captured_frames_appended(0);
        let previous_len = state.captured_frames.len();

        state.captured_frames.push([0.25, -0.4]);
        state.captured_frames_appended(previous_len);

        let preview = state.preview_for_track(track).unwrap();
        assert_eq!(preview.frames_per_peak, LIVE_WAVEFORM_FRAMES_PER_PEAK * 2);
        assert_eq!(preview.peaks.len(), LIVE_WAVEFORM_MAX_PEAKS / 2 + 1);
        assert_eq!(preview.peaks.last(), Some(&(-0.4, 0.25)));
    }
}
