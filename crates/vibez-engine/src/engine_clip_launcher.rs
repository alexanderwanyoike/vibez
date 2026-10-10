//! Independent per-track clocks share the normal channel-strip renderer.

use super::*;
use crate::events::{ClipPlayingState, ClipQueuedState, ClipTrackState};
use crate::playback_source::{ActiveClipPlayback, PreparedClipPlayback};

impl AudioEngine {
    pub(super) fn edit_clip(
        &mut self,
        active: Box<PreparedClipPlayback>,
        mut queued: Box<PreparedClipPlayback>,
    ) {
        self.clip_through_request = self
            .clip_through_request
            .max(active.request_id)
            .max(queued.request_id);
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == active.track_id) {
            if track
                .active_clip
                .is_some_and(|playing| Some(playing.clip_id) == active.clip_id)
            {
                track.release_edited_launcher_notes(&active.source);
            }
            if let Some(pending) = track
                .queued_clip
                .as_mut()
                .filter(|pending| pending.prepared.clip_id == queued.clip_id)
            {
                std::mem::swap(&mut pending.prepared, &mut queued);
            }
        }
        self.clip_event(EngineEvent::ClipRequestRetired(queued));
        self.refresh_clip(active);
    }

    pub(super) fn clip_event(&mut self, event: EngineEvent) {
        let physical = self.output_position + self.rendered_callback_frames as u64;
        self.clip_event_at(event, physical);
    }

    pub(super) fn clip_event_at(&mut self, event: EngineEvent, physical: u64) {
        let request = match &event {
            EngineEvent::ClipQueued { request_id, .. }
            | EngineEvent::ClipTransitioned { request_id, .. }
            | EngineEvent::ClipSourceRefreshed { request_id, .. } => *request_id,
            EngineEvent::ClipRequestRetired(prepared) => prepared.request_id,
            _ => 0,
        };
        self.clip_through_request = self.clip_through_request.max(request);
        if let Err(rtrb::PushError::Full(event)) = self.event_tx.push(event) {
            self.clip_event_drops = self.clip_event_drops.saturating_add(1);
            self.clip_resync_track = Some(0);
            self.present_event_at(event, physical);
        }
    }

    pub(super) fn resync_clip_events(&mut self) {
        if self.reported_clip_event_drops != self.clip_event_drops {
            if self
                .event_tx
                .push(EngineEvent::ClipEventsDropped {
                    total: self.clip_event_drops,
                })
                .is_err()
            {
                return;
            }
            self.reported_clip_event_drops = self.clip_event_drops;
        }
        while let Some(index) = self.clip_resync_track {
            let Some(track) = self.tracks.get(index) else {
                self.clip_resync_track = None;
                break;
            };
            let state = ClipTrackState {
                track_id: track.id,
                playing: track.active_clip.map(|active| ClipPlayingState {
                    clip_id: active.clip_id,
                    request_id: active.request_id,
                    position: active.position,
                }),
                queued: track.queued_clip.as_ref().map(|queued| ClipQueuedState {
                    clip_id: queued.prepared.clip_id,
                    request_id: queued.prepared.request_id,
                }),
                through_request: self.clip_through_request,
                effective_at_samples: self.performance_position,
                running: self.clip_performance && self.transport.is_playing(),
                transport_playing: self.transport.is_playing(),
            };
            // Partial retries must survive even when telemetry fills a one-slot ring.
            if self
                .event_tx
                .push(EngineEvent::ClipStateResynced(state))
                .is_err()
            {
                return;
            }
            self.clip_resync_track = Some(index + 1);
        }
    }

    pub(super) fn begin_clip_performance(&mut self) {
        if !self.clip_performance {
            self.begin_performance_clock();
            self.clip_performance = true;
            self.stopped_note_repeat_anchor = None;
            self.reanchor_note_repeats(self.performance_position, self.performance_position);
            for track in &mut self.tracks {
                track.flush_notes();
            }
        }
        self.transport.set_audio_length(None);
        if !self.transport.is_playing() {
            self.transport.play();
            self.clip_event(EngineEvent::PlaybackStarted);
        }
    }

    pub(super) fn clear_clip_performance(&mut self) {
        if let Some(batch) = self.pending_clip_batch.take() {
            self.clip_event(EngineEvent::ClipBatchRetired(batch.clips));
        }
        self.stop_clip_record(true);
        for index in 0..self.tracks.len() {
            if let Some(queued) = self.tracks[index].queued_clip.take() {
                self.clip_event(EngineEvent::ClipRequestRetired(queued.prepared));
            }
            self.tracks[index].active_clip = None;
        }
        self.clip_performance = false;
    }

    pub(super) fn apply_clip_boundaries(&mut self, now: u64) {
        self.continue_clip_batch(now);
        for index in 0..self.tracks.len() {
            let track = &self.tracks[index];
            let queued_due = track
                .queued_clip
                .as_ref()
                .is_some_and(|queued| !queued.pending_batch && queued.effective_at <= now);
            let ended = track
                .active_clip
                .is_some_and(|clip| !clip.looping && clip.position >= clip.length);
            if (queued_due || ended) && !self.presentation_room(2) {
                break;
            }
            let track = &mut self.tracks[index];
            if track
                .queued_clip
                .as_ref()
                .is_some_and(|queued| !queued.pending_batch && queued.effective_at <= now)
            {
                let mut prepared = track.queued_clip.take().expect("due Clip").prepared;
                track.flush_notes();
                std::mem::swap(&mut track.launcher_source, &mut prepared.source);
                track.active_clip = prepared.clip_id.map(|clip_id| ActiveClipPlayback {
                    clip_id,
                    request_id: prepared.request_id,
                    position: 0,
                    length: prepared.length_samples.max(1),
                    looping: prepared.looping,
                });
                let event = EngineEvent::ClipTransitioned {
                    track_id: prepared.track_id,
                    clip_id: prepared.clip_id,
                    request_id: prepared.request_id,
                    effective_at_samples: now,
                    retired: Some(prepared),
                };
                self.clip_event(event);
            } else if track
                .active_clip
                .is_some_and(|active| active.position >= active.length)
            {
                let active = track.active_clip.as_mut().expect("ended Clip");
                if active.looping {
                    active.position = 0;
                    track.flush_notes();
                } else {
                    track.active_clip = None;
                    track.flush_notes();
                    let event = EngineEvent::ClipTransitioned {
                        track_id: track.id,
                        clip_id: None,
                        request_id: 0,
                        effective_at_samples: now,
                        retired: None,
                    };
                    self.clip_event(event);
                }
            }
        }
    }

    pub(super) fn process_clip_multitrack(
        &mut self,
        output: &mut [f32],
        frames: usize,
        channels: usize,
        live_input: Option<LiveInputBlock<'_>>,
        mut capture: Option<&mut TrackOutputCapture<'_>>,
    ) {
        let physical_offset = self.rendered_callback_frames;
        let mut rendered = 0;
        while rendered < frames {
            let now = self.performance_position + rendered as u64;
            self.rendered_callback_frames = physical_offset + rendered;
            self.apply_clip_record_boundary(now);
            self.apply_clip_boundaries(now);
            let mut count = frames - rendered;
            if let Some(boundary) = self.next_clip_record_boundary() {
                count = count.min(boundary.saturating_sub(now) as usize);
            }
            for track in &self.tracks {
                if let Some(queued) = track
                    .queued_clip
                    .as_ref()
                    .filter(|queued| !queued.pending_batch || self.clip_batch_waiting_boundary())
                {
                    count = count.min(queued.effective_at.saturating_sub(now) as usize);
                }
                if let Some(active) = track.active_clip {
                    count = count.min(active.length.saturating_sub(active.position) as usize);
                }
            }
            debug_assert!(count > 0);
            let count = clip_segment_frames(count, frames - rendered);
            let count_in = self.clip_count_in_timing();
            for track in &mut self.tracks {
                // An inactive slot is silent while live instruments and effect tails still render.
                if track.active_clip.is_some() {
                    std::mem::swap(&mut track.playback_source, &mut track.launcher_source);
                } else {
                    std::mem::swap(&mut track.playback_source, &mut track.empty_launcher_source);
                }
            }
            self.render_multitrack_segment(
                &mut output[rendered * channels..(rendered + count) * channels],
                super::render_paths::MultitrackRenderBlock {
                    pos: 0,
                    repeat_pos: now,
                    frames: count,
                    channels,
                    loop_region: None,
                    live_input: live_input.map(|input| LiveInputBlock {
                        target_track_raw: input.target_track_raw,
                        samples: &input.samples[rendered * channels..(rendered + count) * channels],
                    }),
                },
                capture
                    .as_mut()
                    .map(|tap| TrackOutputCapture {
                        source_track_raw: tap.source_track_raw,
                        samples: &mut tap.samples
                            [rendered * channels..(rendered + count) * channels],
                    })
                    .as_mut(),
            );
            for track in &mut self.tracks {
                if let Some(active) = &mut track.active_clip {
                    active.position += count as u64;
                    std::mem::swap(&mut track.playback_source, &mut track.launcher_source);
                } else {
                    std::mem::swap(&mut track.playback_source, &mut track.empty_launcher_source);
                }
            }
            if let Some(timing) = count_in {
                self.mix_record_count_in_click(
                    &mut output[rendered * channels..(rendered + count) * channels],
                    count,
                    channels,
                    now,
                    timing,
                );
            }
            rendered += count;
            self.rendered_callback_frames = physical_offset + rendered;
        }
    }
}

fn clip_segment_frames(count: usize, remaining: usize) -> usize {
    // A broken boundary invariant must not stall the release audio callback.
    if count == 0 {
        remaining
    } else {
        count
    }
}

#[cfg(test)]
mod segment_tests {
    use super::clip_segment_frames;

    #[test]
    fn zero_length_segment_renders_the_remainder_without_spinning() {
        for remaining in [1, 32, 512] {
            assert_eq!(clip_segment_frames(0, remaining), remaining);
            for count in [1, remaining] {
                assert_eq!(clip_segment_frames(count, remaining), count);
            }
        }
    }
}
