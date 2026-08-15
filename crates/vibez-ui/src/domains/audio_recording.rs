//! Runtime lifecycle for one Arrange hardware-input recording target.

use vibez_core::id::TrackId;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AudioRecordingPhase {
    #[default]
    Idle,
    Recording,
    Stopping,
    Finalizing,
}

#[derive(Debug, Default)]
pub struct AudioRecordingState {
    pub armed_track: Option<TrackId>,
    pub monitor_track: Option<TrackId>,
    pub phase: AudioRecordingPhase,
    pub start_position_samples: u64,
    pub captured_frames: Vec<[f32; 2]>,
    pub input_peak_l: f32,
    pub input_peak_r: f32,
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

    pub fn begin(&mut self, position_samples: u64) -> bool {
        if self.armed_track.is_none() || self.phase != AudioRecordingPhase::Idle {
            return false;
        }
        self.captured_frames.clear();
        self.start_position_samples = position_samples;
        self.phase = AudioRecordingPhase::Recording;
        true
    }

    pub fn request_stop(&mut self) -> bool {
        if self.phase != AudioRecordingPhase::Recording {
            return false;
        }
        self.phase = AudioRecordingPhase::Stopping;
        true
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
        assert!(state.begin(4_800));
        state.captured_frames.push([0.2, -0.2]);
        assert!(state.request_stop());
        let (target, start, take) = state.begin_finalizing().unwrap();
        assert_eq!((target, start), (track, 4_800));
        assert_eq!(take, vec![[0.2, -0.2]]);
        state.finish();
        assert_eq!(state.phase, AudioRecordingPhase::Idle);
    }
}
