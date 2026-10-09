use super::*;
use vibez_core::routing::{
    adapt_channels, EdgeKind, ExternalInputBlock, ExternalInputId, NodeStage,
};

impl AudioEngine {
    pub(super) fn return_retired_routing(&mut self) {
        if let Some(retired) = self.retired_routing.take() {
            if let Err(rtrb::PushError::Full(EngineEvent::RoutingRetired(retired))) =
                self.event_tx.push(EngineEvent::RoutingRetired(retired))
            {
                self.retired_routing = Some(retired);
            }
        }
    }

    pub(super) fn render_routing_graph(
        &mut self,
        output: &mut [f32],
        block: render_paths::MultitrackRenderBlock<'_>,
        mut capture: Option<&mut TrackOutputCapture<'_>>,
        idle: bool,
        capture_audible_only: bool,
    ) {
        if let Some(max_frames) = self
            .routing
            .as_ref()
            .map(|plan| plan.max_frames)
            .filter(|max_frames| block.frames > *max_frames)
        {
            let mut offset = 0;
            while offset < block.frames {
                let frames = (block.frames - offset).min(max_frames);
                let start = offset * block.channels;
                let end = (offset + frames) * block.channels;
                let mut part_capture = capture.as_deref_mut().map(|capture| TrackOutputCapture {
                    source_track_raw: capture.source_track_raw,
                    samples: &mut capture.samples[start..end],
                });
                self.render_routing_graph(
                    &mut output[start..end],
                    render_paths::MultitrackRenderBlock {
                        pos: block.pos + offset as u64,
                        repeat_pos: block.repeat_pos + offset as u64,
                        frames,
                        channels: block.channels,
                        loop_region: block.loop_region,
                        live_input: block
                            .live_input
                            .map(|input| input.slice(offset, frames, block.channels)),
                    },
                    part_capture.as_mut(),
                    idle,
                    capture_audible_only,
                );
                offset += frames;
            }
            return;
        }
        let Some(mut prepared) = self.routing.take() else {
            return;
        };
        let frames = block.frames;
        let channels = block.channels;
        if frames > prepared.max_frames || !matches!(channels, 1 | 2) {
            output.fill(0.0);
            self.routing = Some(prepared);
            return;
        }
        let len = frames * channels;
        let track_solo = any_solo(&self.tracks);
        let bus_solo = any_solo(&self.buses);
        let tempo = TempoMap::new(self.transport.bpm(), self.sample_rate);
        for order_index in 0..prepared.graph.order.len() {
            let index = prepared.graph.order[order_index];
            let node = prepared.graph.nodes[index];
            prepared.nodes[index].samples[..len].fill(0.0);
            match node.stage {
                NodeStage::Source => {
                    let Some(track) = self
                        .tracks
                        .iter_mut()
                        .find(|track| track.id == node.channel)
                    else {
                        continue;
                    };
                    std::mem::swap(&mut track.mix_buffer, &mut prepared.nodes[index].samples);
                    let pos = if self.clip_performance {
                        track
                            .active_clip
                            .map_or(0, |clip| clip.position.saturating_add(block.pos))
                    } else {
                        block.pos
                    };
                    track.apply_graph_automation(pos as f64 / tempo.samples_per_beat());
                    let id = track.id;
                    let section = self.active_section;
                    let events = &mut self.event_tx;
                    let mut repeated = |trigger: crate::note_repeat::NoteRepeatTrigger| {
                        let section_position = section.map(|active| {
                            section_record::section_sample_for_performance(
                                pos,
                                block.repeat_pos,
                                trigger.effective_at_samples,
                                active.length_samples,
                            )
                        });
                        let canonical_section_position = section.map(|active| {
                            section_record::section_sample_for_performance(
                                pos,
                                block.repeat_pos,
                                trigger.canonical_at_samples,
                                active.length_samples,
                            )
                        });
                        let _ = events.push(EngineEvent::NoteRepeated {
                            track_id: id,
                            pitch: trigger.pitch,
                            velocity: trigger.velocity,
                            rate: trigger.rate,
                            effective_at_samples: trigger.effective_at_samples,
                            canonical_at_samples: trigger.canonical_at_samples,
                            section_id: section.map(|active| active.section_id),
                            section_position_samples: section_position,
                            canonical_section_position_samples: canonical_section_position,
                        });
                    };
                    if track.instrument.is_some() {
                        if idle {
                            track.render_instrument_idle(
                                block.repeat_pos,
                                frames,
                                channels,
                                &tempo,
                                self.project_swing,
                                &mut repeated,
                            );
                        } else {
                            track.render_instrument(
                                InstrumentRenderContext {
                                    pos,
                                    repeat_pos: block.repeat_pos,
                                    frames,
                                    channels,
                                    tempo_map: &tempo,
                                    project_swing: self.project_swing,
                                },
                                &mut repeated,
                            );
                        }
                    } else if idle {
                        track.clear_buffer(frames, channels);
                    } else {
                        track.render(pos, frames, channels, block.loop_region);
                    }
                    if let Some(input) = block
                        .live_input
                        .filter(|input| input.target_track_raw == id.raw())
                    {
                        for (sample, live) in track.mix_buffer[..len].iter_mut().zip(input.samples)
                        {
                            *sample += live;
                        }
                    }
                    std::mem::swap(&mut track.mix_buffer, &mut prepared.nodes[index].samples);
                    let activity = track.take_note_activity();
                    if activity != 0 {
                        let _ = self.event_tx.push(EngineEvent::TrackNoteActivity {
                            track_id: id,
                            triggered_notes: activity,
                        });
                    }
                }
                NodeStage::Sum => {
                    let track = if node.channel.is_master() {
                        Some(&mut self.master)
                    } else {
                        self.buses.iter_mut().find(|bus| bus.id == node.channel)
                    };
                    let Some(track) = track else {
                        continue;
                    };
                    track.apply_graph_automation(block.pos as f64 / tempo.samples_per_beat());
                    for edge in prepared.graph.edges.iter().filter(|edge| edge.to == index) {
                        let audible = match edge.kind {
                            EdgeKind::Mix => {
                                if let Some(track) = self.tracks.iter().find(|track| {
                                    track.id == prepared.graph.nodes[edge.from].channel
                                }) {
                                    (!track_solo && !bus_solo) || track.solo
                                } else {
                                    self.buses
                                        .iter()
                                        .find(|bus| {
                                            bus.id == prepared.graph.nodes[edge.from].channel
                                        })
                                        .is_some_and(|bus| {
                                            (!bus_solo || bus.solo)
                                                && (!track_solo
                                                    || bus.solo
                                                    || !prepared.detector_buses.contains(&bus.id))
                                        })
                                }
                            }
                            EdgeKind::Send(_) => {
                                !track_solo
                                    || bus_solo
                                    || prepared.detector_buses.contains(&node.channel)
                                    || self
                                        .tracks
                                        .iter()
                                        .chain(self.buses.iter())
                                        .find(|channel| {
                                            channel.id == prepared.graph.nodes[edge.from].channel
                                        })
                                        .is_some_and(|channel| channel.solo)
                            }
                            _ => true,
                        };
                        if !audible {
                            continue;
                        }
                        let gain = match edge.kind {
                            EdgeKind::Send(_) => self
                                .tracks
                                .iter()
                                .chain(self.buses.iter())
                                .find(|track| track.id == prepared.graph.nodes[edge.from].channel)
                                .map_or(0.0, |track| {
                                    let pos = if self.clip_performance {
                                        track.active_clip.map_or(0, |clip| {
                                            clip.position.saturating_add(block.pos)
                                        })
                                    } else {
                                        block.pos
                                    };
                                    track.effective_send_amount(
                                        node.channel,
                                        pos as f64 / tempo.samples_per_beat(),
                                    )
                                }),
                            _ => 1.0,
                        };
                        for sample in 0..len {
                            prepared.nodes[index].samples[sample] +=
                                prepared.nodes[edge.from].samples[sample] * gain;
                        }
                    }
                }
                NodeStage::Effect(effect_id) => {
                    for edge in prepared
                        .graph
                        .edges
                        .iter()
                        .filter(|edge| edge.to == index && edge.kind == EdgeKind::Main)
                    {
                        for sample in 0..len {
                            prepared.nodes[index].samples[sample] +=
                                prepared.nodes[edge.from].samples[sample];
                        }
                    }
                    let (before, remaining) = prepared.nodes.split_at_mut(index);
                    let (destination, after) = remaining.split_first_mut().unwrap();
                    for input in &mut destination.inputs {
                        if let Some(source) = input.source {
                            let samples = if source < index {
                                &before[source].samples
                            } else {
                                &after[source - index - 1].samples
                            };
                            adapt_channels(
                                &samples[..len],
                                channels,
                                &mut input.samples[..frames * input.descriptor.channels],
                                input.descriptor.channels,
                            );
                        } else {
                            input.samples[..frames * input.descriptor.channels].fill(0.0);
                        }
                    }
                    let empty = ExternalInputBlock {
                        id: ExternalInputId(0),
                        channels: 2,
                        samples: &[],
                        connected: false,
                    };
                    let mut blocks = [empty; crate::routing::MAX_EXTERNAL_INPUTS];
                    for (slot, input) in blocks.iter_mut().zip(&destination.inputs) {
                        *slot = ExternalInputBlock {
                            id: input.descriptor.id,
                            channels: input.descriptor.channels,
                            samples: &input.samples[..frames * input.descriptor.channels],
                            connected: input.connected,
                        };
                        let levels = metering::calculate_meters(slot.samples, slot.channels);
                        let _ = self.event_tx.push(EngineEvent::SidechainInputMeter {
                            effect_id,
                            input_id: slot.id,
                            peak_l: levels.peak_l,
                            peak_r: levels.peak_r,
                        });
                    }
                    let track = if node.channel.is_master() {
                        Some(&mut self.master)
                    } else {
                        self.tracks
                            .iter_mut()
                            .chain(self.buses.iter_mut())
                            .find(|track| track.id == node.channel)
                    };
                    if let Some(track) = track {
                        if let Some(slot) = track
                            .effects
                            .iter_mut()
                            .find(|slot| slot.id == effect_id)
                            .filter(|slot| !slot.bypass)
                        {
                            slot.effect.process_with_inputs(
                                &mut destination.samples[..len],
                                channels,
                                &blocks[..destination.inputs.len()],
                            );
                        }
                    }
                }
                NodeStage::AfterEffects | NodeStage::AfterFader => {
                    if let Some(edge) = prepared
                        .graph
                        .edges
                        .iter()
                        .find(|edge| edge.to == index && edge.kind == EdgeKind::Main)
                    {
                        for sample in 0..len {
                            prepared.nodes[index].samples[sample] =
                                prepared.nodes[edge.from].samples[sample];
                        }
                    }
                    let bus_channel = node.channel.is_master()
                        || self.buses.iter().any(|bus| bus.id == node.channel);
                    let track = if node.channel.is_master() {
                        Some(&mut self.master)
                    } else {
                        self.tracks
                            .iter_mut()
                            .chain(self.buses.iter_mut())
                            .find(|track| track.id == node.channel)
                    };
                    if let Some(track) = track {
                        if node.stage == NodeStage::AfterFader {
                            let pos = if self.clip_performance {
                                track
                                    .active_clip
                                    .map_or(0, |clip| clip.position.saturating_add(block.pos))
                            } else {
                                block.pos
                            };
                            let (gain, pan) =
                                track.automation_mix_values(pos as f64 / tempo.samples_per_beat());
                            let gain = gain.unwrap_or(track.gain);
                            let pan = pan.unwrap_or(track.pan);
                            let (left, right) = if bus_channel {
                                crate::mixer::balance_pan(pan)
                            } else {
                                equal_power_pan(pan)
                            };
                            std::mem::swap(
                                &mut track.mix_buffer,
                                &mut prepared.nodes[index].samples,
                            );
                            track.apply_mute_envelope(
                                pos,
                                frames,
                                channels,
                                tempo.samples_per_beat(),
                            );
                            for frame in 0..frames {
                                for channel in 0..channels {
                                    let offset = frame * channels + channel;
                                    track.mix_buffer[offset] = track.mix_buffer[offset]
                                        * gain
                                        * if channels == 1 {
                                            1.0
                                        } else if channel == 0 {
                                            left
                                        } else {
                                            right
                                        };
                                }
                            }
                            std::mem::swap(
                                &mut track.mix_buffer,
                                &mut prepared.nodes[index].samples,
                            );
                            let levels = metering::calculate_meters(
                                &prepared.nodes[index].samples[..len],
                                channels,
                            );
                            let _ = self.event_tx.push(EngineEvent::TrackMeter {
                                track_id: node.channel,
                                peak_l: levels.peak_l,
                                peak_r: levels.peak_r,
                            });
                            if let Some(capture) = capture
                                .as_deref_mut()
                                .filter(|capture| capture.source_track_raw == node.channel.raw())
                            {
                                let audible = if bus_channel {
                                    (!bus_solo || track.solo)
                                        && (!track_solo
                                            || track.solo
                                            || !prepared.detector_buses.contains(&node.channel))
                                } else {
                                    (!track_solo && !bus_solo) || track.solo
                                };
                                if !capture_audible_only || audible {
                                    capture.samples[..len]
                                        .copy_from_slice(&prepared.nodes[index].samples[..len]);
                                } else {
                                    capture.samples[..len].fill(0.0);
                                }
                            }
                            if node.channel.is_master() {
                                output[..len]
                                    .copy_from_slice(&prepared.nodes[index].samples[..len]);
                            }
                        }
                        if self.spectrum_track == Some(node.channel)
                            && node.stage == NodeStage::AfterEffects
                        {
                            push_spectrum(
                                &mut self.spectrum_tx,
                                &prepared.nodes[index].samples[..len],
                                channels,
                            );
                        }
                    }
                }
            }
        }
        self.routing = Some(prepared);
    }
}
