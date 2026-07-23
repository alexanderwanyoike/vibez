//! Resident Section queue ownership and sample-accurate callback switching.

use super::*;
use vibez_core::perform::SectionLaunchQuantization;

impl AudioEngine {
    pub(super) fn activate_section(
        &mut self,
        mut prepared: Box<PreparedSectionPlaybackSource>,
        effective_at_samples: u64,
    ) {
        self.begin_performance_clock();
        let section_id = prepared.section_id;
        let length_samples = self.section_length_samples(prepared.length_beats);
        let looping = prepared.looping;
        for track in &mut self.tracks {
            track.flush_notes();
        }
        for incoming in prepared.tracks_mut() {
            if let Some(track) = self
                .tracks
                .iter_mut()
                .find(|track| track.id == incoming.track_id)
            {
                std::mem::swap(&mut track.section_playback_source, &mut incoming.source);
            }
        }
        self.active_section = Some(ActiveSectionPlayback {
            section_id,
            position_samples: 0,
            length_samples,
            looping,
        });
        self.transport.set_audio_length(None);
        if !self.transport.is_playing() {
            self.transport.play();
            let _ = self.event_tx.push(EngineEvent::PlaybackStarted);
        }
        self.stopped_note_repeat_anchor = None;
        self.reanchor_note_repeats(effective_at_samples, effective_at_samples);
        let event = EngineEvent::SectionTransitioned {
            section_id,
            effective_at_samples,
            retired: prepared,
        };
        if let Err(rtrb::PushError::Full(event)) = self.event_tx.push(event) {
            // Never destroy Vec/Arc owners in the callback. Losing this rare
            // event leaks one retired source rather than glitching.
            std::mem::forget(event);
        }
    }

    pub(super) fn queue_section(
        &mut self,
        prepared: Box<PreparedSectionPlaybackSource>,
        quantization: SectionLaunchQuantization,
    ) {
        if self.pending_section_record.is_some() || self.active_section_record.is_some() {
            let event = EngineEvent::SectionQueueCancelled { retired: prepared };
            if let Err(rtrb::PushError::Full(event)) = self.event_tx.push(event) {
                std::mem::forget(event);
            }
            return;
        }
        self.begin_performance_clock();
        let now = self.performance_position;
        if quantization == SectionLaunchQuantization::Immediate
            || self.active_section.is_none()
            || !self.transport.is_playing()
        {
            self.cancel_section_queue();
            self.activate_section(prepared, now);
            return;
        }

        let effective_at_samples = match quantization {
            SectionLaunchQuantization::Immediate => now,
            SectionLaunchQuantization::OneBeat => self.next_grid_boundary(now, 1.0),
            SectionLaunchQuantization::OneBar => self.next_grid_boundary(now, 4.0),
            SectionLaunchQuantization::EndOfSection => self
                .active_section
                .map(|active| {
                    now.saturating_add(
                        active
                            .length_samples
                            .saturating_sub(active.position_samples),
                    )
                })
                .unwrap_or(now),
        };

        if effective_at_samples <= now {
            self.cancel_section_queue();
            self.activate_section(prepared, now);
            return;
        }

        let section_id = prepared.section_id;
        let retired = self.queued_section.replace(QueuedSectionPlayback {
            prepared,
            effective_at_samples,
        });
        let event = EngineEvent::SectionQueued {
            section_id,
            effective_at_samples,
            retired: retired.map(|queued| queued.prepared),
        };
        if let Err(rtrb::PushError::Full(event)) = self.event_tx.push(event) {
            std::mem::forget(event);
        }
    }

    pub(super) fn cancel_section_queue(&mut self) {
        let Some(queued) = self.queued_section.take() else {
            return;
        };
        let event = EngineEvent::SectionQueueCancelled {
            retired: queued.prepared,
        };
        if let Err(rtrb::PushError::Full(event)) = self.event_tx.push(event) {
            std::mem::forget(event);
        }
    }

    pub(super) fn process_section_multitrack(
        &mut self,
        output: &mut [f32],
        frames: usize,
        channels: usize,
    ) {
        let block_start = self.performance_position;
        let block_end = block_start.saturating_add(frames as u64);
        let boundary = self
            .queued_section
            .as_ref()
            .map(|queued| queued.effective_at_samples);

        if boundary.is_some_and(|boundary| boundary <= block_start) {
            let queued = self.queued_section.take().expect("queued Section");
            self.activate_section(queued.prepared, block_start);
        }

        if let Some(boundary) =
            boundary.filter(|boundary| *boundary > block_start && *boundary < block_end)
        {
            let frames_before = (boundary - block_start) as usize;
            let old_section = self.active_section.expect("active Section");
            self.render_section_frames(
                &mut output[..frames_before * channels],
                frames_before,
                channels,
                old_section,
                block_start,
            );
            let queued = self.queued_section.take().expect("queued Section");
            self.activate_section(queued.prepared, boundary);
            let frames_after = frames - frames_before;
            let new_section = self.active_section.expect("active Section");
            self.render_section_frames(
                &mut output[frames_before * channels..],
                frames_after,
                channels,
                new_section,
                boundary,
            );
            self.section_advance_override = Some(frames_after as u64);
            return;
        }

        let section = self.active_section.expect("active Section");
        self.render_section_frames(output, frames, channels, section, block_start);
    }

    pub(super) fn render_section_frames(
        &mut self,
        output: &mut [f32],
        frames: usize,
        channels: usize,
        section: ActiveSectionPlayback,
        performance_position: u64,
    ) {
        for track in &mut self.tracks {
            std::mem::swap(
                &mut track.playback_source,
                &mut track.section_playback_source,
            );
        }

        let mut rendered_frames = 0usize;
        let mut local_position = section.position_samples.min(section.length_samples);
        while rendered_frames < frames && local_position < section.length_samples {
            let available = (section.length_samples - local_position) as usize;
            let mut segment_frames = available.min(frames - rendered_frames);
            let segment_performance_position = performance_position + rendered_frames as u64;
            if let Some(record) = self.active_section_record.filter(|record| {
                record.replace_first_pass
                    && segment_performance_position < record.effective_at_samples
            }) {
                let until_replace = record
                    .effective_at_samples
                    .saturating_sub(segment_performance_position)
                    as usize;
                segment_frames = segment_frames.min(until_replace);
            }
            let replace_track = self
                .active_section_record
                .filter(|record| {
                    record.replace_first_pass
                        && segment_performance_position >= record.effective_at_samples
                })
                .map(|record| record.track_id);
            if let Some(track_id) = replace_track {
                let needs_flush = self
                    .active_section_record
                    .is_some_and(|record| !record.replace_source_flushed);
                if needs_flush {
                    if let Some(track) = self.tracks.iter_mut().find(|track| track.id == track_id) {
                        track.flush_notes();
                    }
                    if let Some(record) = self.active_section_record.as_mut() {
                        record.replace_source_flushed = true;
                    }
                }
            }
            for track in &mut self.tracks {
                track.suppress_source_notes = Some(track.id) == replace_track;
            }
            let start = rendered_frames * channels;
            let end = (rendered_frames + segment_frames) * channels;
            self.render_multitrack_segment(
                &mut output[start..end],
                local_position,
                segment_performance_position,
                segment_frames,
                channels,
                None,
            );
            for track in &mut self.tracks {
                track.suppress_source_notes = false;
            }
            rendered_frames += segment_frames;
            local_position += segment_frames as u64;
            if local_position >= section.length_samples && replace_track.is_some() {
                if let Some(record) = self.active_section_record.as_mut() {
                    record.replace_first_pass = false;
                }
            }
            if rendered_frames < frames
                && local_position >= section.length_samples
                && section.looping
            {
                for track in &mut self.tracks {
                    track.flush_notes();
                }
                local_position = 0;
            } else if local_position >= section.length_samples {
                break;
            }
        }

        for track in &mut self.tracks {
            std::mem::swap(
                &mut track.playback_source,
                &mut track.section_playback_source,
            );
        }
    }

    pub(super) fn section_length_samples(&self, length_beats: f64) -> u64 {
        if self.transport.bpm() > 0.0 {
            (length_beats * self.sample_rate as f64 * 60.0 / self.transport.bpm())
                .round()
                .max(1.0) as u64
        } else {
            1
        }
    }

    fn next_grid_boundary(&self, now: u64, beats: f64) -> u64 {
        if self.transport.bpm() <= 0.0 {
            return now;
        }
        let grid_samples = beats * self.sample_rate as f64 * 60.0 / self.transport.bpm();
        if grid_samples <= 0.0 {
            return now;
        }
        ((now as f64 / grid_samples).ceil() * grid_samples)
            .round()
            .max(now as f64) as u64
    }
}
