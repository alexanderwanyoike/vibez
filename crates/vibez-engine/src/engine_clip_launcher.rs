//! Independent per-track clocks share the normal channel-strip renderer.

use super::*;
use crate::playback_source::{ActiveClipPlayback, PreparedClipPlayback, QueuedClipPlayback};
use vibez_core::perform::MusicalBoundary;

impl AudioEngine {
    pub(super) fn edit_clip(
        &mut self,
        active: Box<PreparedClipPlayback>,
        mut queued: Box<PreparedClipPlayback>,
    ) {
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
        if let Err(rtrb::PushError::Full(event)) = self.event_tx.push(event) {
            // Source owners must be reclaimed on the UI thread, never in the callback.
            std::mem::forget(event);
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
            let _ = self.event_tx.push(EngineEvent::PlaybackStarted);
        }
    }

    // Each owner moves into a track queue without allocating in the callback.
    #[allow(clippy::vec_box)]
    pub(super) fn queue_clips(
        &mut self,
        mut clips: Vec<Box<PreparedClipPlayback>>,
        quantization: MusicalBoundary,
    ) {
        let starting = !self.clip_performance || !self.transport.is_playing();
        self.begin_clip_performance();
        let now = self.performance_position;
        let boundary = if starting {
            now
        } else {
            quantization
                .beats()
                .map_or(now, |beats| self.next_grid_boundary(now, beats))
        };
        while let Some(prepared) = clips.pop() {
            let Some(index) = self
                .tracks
                .iter()
                .position(|track| track.id == prepared.track_id)
            else {
                self.clip_event(EngineEvent::ClipRequestRetired(prepared));
                continue;
            };
            let request_id = prepared.request_id;
            let track_id = prepared.track_id;
            let clip_id = prepared.clip_id;
            if let Some(old) = self.tracks[index].queued_clip.replace(QueuedClipPlayback {
                prepared,
                effective_at: boundary,
            }) {
                self.clip_event(EngineEvent::ClipRequestRetired(old.prepared));
            }
            let _ = self.event_tx.push(EngineEvent::ClipQueued {
                request_id,
                track_id,
                clip_id,
            });
        }
        self.clip_event(EngineEvent::ClipBatchRetired(clips));
        self.apply_clip_boundaries(now);
    }

    pub(super) fn clear_clip_performance(&mut self) {
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
        for index in 0..self.tracks.len() {
            let track = &mut self.tracks[index];
            if track
                .queued_clip
                .as_ref()
                .is_some_and(|queued| queued.effective_at <= now)
            {
                let mut prepared = track.queued_clip.take().expect("due Clip").prepared;
                track.flush_notes();
                std::mem::swap(&mut track.launcher_source, &mut prepared.source);
                track.active_clip = prepared.clip_id.map(|clip_id| ActiveClipPlayback {
                    clip_id,
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
        let mut rendered = 0;
        while rendered < frames {
            let now = self.performance_position + rendered as u64;
            self.apply_clip_record_boundary(now);
            self.apply_clip_boundaries(now);
            let mut count = frames - rendered;
            if let Some(boundary) = self.next_clip_record_boundary() {
                count = count.min(boundary.saturating_sub(now) as usize);
            }
            for track in &self.tracks {
                if let Some(queued) = &track.queued_clip {
                    count = count.min(queued.effective_at.saturating_sub(now) as usize);
                }
                if let Some(active) = track.active_clip {
                    count = count.min(active.length.saturating_sub(active.position) as usize);
                }
            }
            let count = count.max(1);
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
        }
    }
}
