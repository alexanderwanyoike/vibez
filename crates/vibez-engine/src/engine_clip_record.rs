//! Clip recording boundaries belong to the Perform clock; source ownership stays off the callback.

use super::*;
use crate::playback_source::{PreparedClipPlayback, QueuedClipPlayback};
use vibez_core::id::ClipId;

pub(super) struct ClipRecordRuntime {
    free_length: bool,
    clip_id: ClipId,
    track_id: TrackId,
    start: u64,
    count_in_start: u64,
    stop: Option<u64>,
    started: bool,
    prepared: Option<Box<PreparedClipPlayback>>,
}

impl AudioEngine {
    pub(super) fn arm_clip_record(
        &mut self,
        prepared: Box<PreparedClipPlayback>,
        count_in_bars: u8,
        free_length: bool,
    ) {
        if self.clip_record.is_some() || prepared.clip_id.is_none() {
            self.clip_event(EngineEvent::ClipRequestRetired(prepared));
            return;
        }
        let running = self.clip_performance && self.transport.is_playing();
        if !self
            .tracks
            .iter()
            .any(|track| track.id == prepared.track_id)
        {
            self.clip_event(EngineEvent::ClipRecordStopped {
                clip_id: prepared.clip_id.expect("record target"),
                at: self.performance_position,
                started: false,
            });
            self.clip_event(EngineEvent::ClipRequestRetired(prepared));
            return;
        }
        self.begin_clip_performance();
        let now = self.performance_position;
        let start = if running {
            self.next_grid_boundary(now, 4.0)
        } else {
            now.saturating_add(self.section_length_samples(4.0) * u64::from(count_in_bars))
        };
        let clip_id = prepared.clip_id.expect("record target");
        let track_id = prepared.track_id;
        let output_start = self
            .output_position
            .saturating_add(start.saturating_sub(now));
        self.clip_record = Some(ClipRecordRuntime {
            free_length,
            clip_id,
            track_id,
            start,
            count_in_start: now,
            stop: None,
            started: false,
            prepared: Some(prepared),
        });
        self.clip_event(EngineEvent::ClipRecordArmed {
            clip_id,
            track_id,
            start,
            output_start,
        });
        self.apply_clip_record_boundary(now);
    }

    pub(super) fn clip_count_in_timing(&self) -> Option<super::section_record::CountInClickTiming> {
        self.clip_record
            .as_ref()
            .filter(|record| !record.started)
            .map(|record| super::section_record::CountInClickTiming {
                start: record.count_in_start,
                boundary: record.start,
                beat_samples: self.section_length_samples(1.0),
            })
    }

    pub(super) fn next_clip_record_boundary(&self) -> Option<u64> {
        self.clip_record.as_ref().and_then(|record| {
            if record.started {
                record.stop
            } else {
                Some(record.start)
            }
        })
    }

    pub(super) fn stop_clip_record(&mut self, immediate: bool) {
        let now = self.performance_position;
        let stop = if immediate {
            now
        } else {
            self.next_grid_boundary(now, 4.0)
        };
        let Some(record) = self.clip_record.as_mut() else {
            return;
        };
        record.stop = Some(record.stop.map_or(stop, |pending| pending.min(stop)));
        if immediate || !record.started {
            self.finish_clip_record(now);
        }
    }

    fn finish_clip_record(&mut self, now: u64) {
        let Some(mut record) = self.clip_record.take() else {
            return;
        };
        if let Some(prepared) = record.prepared.take() {
            self.clip_event(EngineEvent::ClipRequestRetired(prepared));
        }
        if record.started && record.free_length {
            if let Some(track) = self
                .tracks
                .iter_mut()
                .find(|track| track.id == record.track_id)
            {
                if let Some(active) = track
                    .active_clip
                    .as_mut()
                    .filter(|active| active.clip_id == record.clip_id)
                {
                    active.length = now.saturating_sub(record.start).max(1);
                    active.position = 0;
                    active.looping = true;
                    track.flush_notes();
                }
            }
        }
        self.clip_event(EngineEvent::ClipRecordStopped {
            clip_id: record.clip_id,
            at: now,
            started: record.started,
        });
    }

    pub(super) fn apply_clip_record_boundary(&mut self, now: u64) {
        let Some(record) = &mut self.clip_record else {
            return;
        };
        if record.started {
            if record.stop.is_some_and(|stop| stop <= now) {
                self.finish_clip_record(now);
            }
            return;
        }
        if now < record.start {
            return;
        }
        let clip_id = record.clip_id;
        let track_id = record.track_id;
        let prepared = record.prepared.take().expect("armed source");
        if let Some(track) = self.tracks.iter_mut().find(|track| track.id == track_id) {
            let retired = track.queued_clip.replace(QueuedClipPlayback {
                prepared,
                effective_at: now,
            });
            if let Some(retired) = retired {
                self.clip_event(EngineEvent::ClipRequestRetired(retired.prepared));
            }
        } else {
            self.clip_event(EngineEvent::ClipRequestRetired(prepared));
            self.finish_clip_record(now);
            return;
        }
        self.clip_record.as_mut().expect("record target").started = true;
        self.apply_clip_boundaries(now);
        self.clip_event(EngineEvent::ClipRecordStarted { clip_id, at: now });
    }

    pub(super) fn refresh_clip(&mut self, mut prepared: Box<PreparedClipPlayback>) {
        let recording_free = self
            .clip_record
            .as_ref()
            .is_some_and(|record| record.free_length && Some(record.clip_id) == prepared.clip_id);
        let mut refreshed = None;
        if let Some(track) = self
            .tracks
            .iter_mut()
            .find(|track| track.id == prepared.track_id)
        {
            if let Some(active) = track
                .active_clip
                .as_mut()
                .filter(|active| Some(active.clip_id) == prepared.clip_id)
            {
                std::mem::swap(&mut track.launcher_source, &mut prepared.source);
                if !recording_free {
                    active.length = prepared.length_samples.max(1);
                    active.looping = prepared.looping;
                    active.position = if active.looping {
                        active.position % active.length
                    } else {
                        active.position.min(active.length)
                    };
                }
                refreshed = Some((track.id, active.position));
            }
        }
        if let Some((track_id, position)) = refreshed {
            self.clip_event(if prepared.request_id == 0 {
                EngineEvent::ClipCaptureSource {
                    track_id,
                    position,
                    effective_at_samples: self.performance_position,
                }
            } else {
                EngineEvent::ClipSourceRefreshed {
                    track_id,
                    request_id: prepared.request_id,
                    position,
                    effective_at_samples: self.performance_position,
                }
            });
        }
        self.clip_event(EngineEvent::ClipRequestRetired(prepared));
    }
}
