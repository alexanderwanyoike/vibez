//! Arrange, Section, and stopped-instrument rendering paths.

use super::*;

#[derive(Clone, Copy)]
pub(super) struct MultitrackRenderBlock<'a> {
    pub pos: u64,
    pub repeat_pos: u64,
    pub frames: usize,
    pub channels: usize,
    pub loop_region: Option<(u64, u64)>,
    pub live_input: Option<LiveInputBlock<'a>>,
}

fn split_track_output_capture<'a>(
    capture: Option<&'a mut TrackOutputCapture<'_>>,
    split: usize,
) -> (
    Option<TrackOutputCapture<'a>>,
    Option<TrackOutputCapture<'a>>,
) {
    let Some(capture) = capture else {
        return (None, None);
    };
    let split = split.min(capture.samples.len());
    let source_track_raw = capture.source_track_raw;
    let (first, rest) = capture.samples.split_at_mut(split);
    (
        Some(TrackOutputCapture {
            source_track_raw,
            samples: first,
        }),
        Some(TrackOutputCapture {
            source_track_raw,
            samples: rest,
        }),
    )
}

impl AudioEngine {
    /// Multi-track rendering: render each track, apply gain/pan, sum into output.
    ///
    /// When the arrangement loop boundary falls inside this block the
    /// work is split into two segments around it. Without the split,
    /// the block renders linearly past the loop end and the next
    /// block starts after the loop start, so note-ons in the skipped
    /// window (e.g. a note right at the loop start) never fire.
    pub(super) fn process_multitrack(
        &mut self,
        output: &mut [f32],
        frames: usize,
        channels: usize,
        live_input: Option<LiveInputBlock<'_>>,
        mut track_output_capture: Option<&mut TrackOutputCapture<'_>>,
    ) {
        if !self.transport.is_playing() {
            // Stopped transport still renders instruments so
            // auditioned notes (piano-roll keys, drum pads) and
            // plugin-queued events are audible, like any DAW.
            self.render_idle_instruments(
                output,
                frames,
                channels,
                self.performance_position,
                live_input,
                track_output_capture,
            );
            return;
        }

        if self.process_section_record_count_in(output, frames, channels) {
            return;
        }

        if self.clip_performance {
            self.process_clip_multitrack(
                output,
                frames,
                channels,
                live_input,
                track_output_capture,
            );
            return;
        }
        if self.active_section.is_some() {
            if frames > 0 {
                self.start_section_record_if_due(
                    self.performance_position
                        .saturating_add(frames as u64)
                        .saturating_sub(1),
                );
            }
            self.process_section_multitrack(output, frames, channels);
            return;
        }

        let pos = self.transport.position();
        if let Some((loop_start, loop_end)) = self.transport.active_loop_region() {
            if pos < loop_end && pos + frames as u64 >= loop_end {
                let first = (loop_end - pos) as usize;
                let rest = frames - first;
                let (mut first_capture, mut rest_capture) = split_track_output_capture(
                    track_output_capture.as_deref_mut(),
                    first * channels,
                );
                self.render_multitrack_segment(
                    &mut output[..first * channels],
                    MultitrackRenderBlock {
                        pos,
                        repeat_pos: pos,
                        frames: first,
                        channels,
                        loop_region: self.transport.active_loop_region(),
                        live_input: live_input.map(|input| input.slice(0, first, channels)),
                    },
                    first_capture.as_mut(),
                );
                // The transport jumps directly from loop_end to loop_start.
                // Apply work owned by that exact musical boundary before the
                // clock wraps so it cannot remain pending forever.
                self.apply_track_mutes_due(loop_end);
                // Kill anything sounding across the boundary, then
                // continue sample-accurately from the loop start.
                for track in &mut self.tracks {
                    track.flush_notes();
                }
                // Held repeats cannot keep deadlines from the later source clock
                // after Arrange returns to an earlier loop position.
                let anchor = self.playing_note_repeat_anchor();
                self.reanchor_note_repeats(anchor, loop_start);
                if rest > 0 {
                    self.render_multitrack_segment(
                        &mut output[first * channels..],
                        MultitrackRenderBlock {
                            pos: loop_start,
                            repeat_pos: loop_start,
                            frames: rest,
                            channels,
                            loop_region: self.transport.active_loop_region(),
                            live_input: live_input.map(|input| input.slice(first, rest, channels)),
                        },
                        rest_capture.as_mut(),
                    );
                }
                self.split_wrap_handled = true;
                return;
            }
        }
        self.render_multitrack_segment(
            output,
            MultitrackRenderBlock {
                pos,
                repeat_pos: pos,
                frames,
                channels,
                loop_region: self.transport.active_loop_region(),
                live_input,
            },
            track_output_capture,
        );
    }

    pub(super) fn render_multitrack_segment(
        &mut self,
        output: &mut [f32],
        block: MultitrackRenderBlock<'_>,
        mut track_output_capture: Option<&mut TrackOutputCapture<'_>>,
    ) {
        let MultitrackRenderBlock {
            pos,
            repeat_pos,
            frames,
            channels,
            loop_region,
            live_input,
        } = block;
        let mut rendered_frames = 0usize;
        while rendered_frames < frames {
            let segment_start = repeat_pos.saturating_add(rendered_frames as u64);
            self.apply_track_mutes_due(segment_start);
            let segment_end = repeat_pos.saturating_add(frames as u64);
            let next_boundary = self
                .next_track_mute_boundary()
                .filter(|boundary| *boundary > segment_start && *boundary < segment_end);
            let segment_frames = next_boundary
                .map(|boundary| (boundary - segment_start) as usize)
                .unwrap_or(frames - rendered_frames);
            let start = rendered_frames * channels;
            let end = (rendered_frames + segment_frames) * channels;
            let mut segment_capture = track_output_capture.as_deref_mut().map(|capture| {
                let samples = capture.samples.get_mut(start..end).unwrap_or(&mut []);
                TrackOutputCapture {
                    source_track_raw: capture.source_track_raw,
                    samples,
                }
            });
            self.render_routing_graph(
                &mut output[start..end],
                MultitrackRenderBlock {
                    pos: pos.saturating_add(rendered_frames as u64),
                    repeat_pos: segment_start,
                    frames: segment_frames,
                    channels,
                    loop_region,
                    live_input: live_input
                        .map(|input| input.slice(rendered_frames, segment_frames, channels)),
                },
                segment_capture.as_mut(),
                false,
                true,
            );
            rendered_frames += segment_frames;
        }
    }

    /// Stopped transport still delivers live notes, queued plugin events and effect tails.
    pub(super) fn render_idle_instruments(
        &mut self,
        output: &mut [f32],
        frames: usize,
        channels: usize,
        repeat_pos: u64,
        live_input: Option<LiveInputBlock<'_>>,
        track_output_capture: Option<&mut TrackOutputCapture<'_>>,
    ) {
        self.render_routing_graph(
            output,
            MultitrackRenderBlock {
                pos: self.transport.position(),
                repeat_pos,
                frames,
                channels,
                loop_region: None,
                live_input,
            },
            track_output_capture,
            true,
            true,
        );
    }
}
