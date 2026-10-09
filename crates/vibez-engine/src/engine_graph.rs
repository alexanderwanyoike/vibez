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
                        pos: if idle {
                            block.pos
                        } else {
                            block.pos + offset as u64
                        },
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
        let continuous_sample = self
            .output_position
            .saturating_add(self.rendered_callback_frames as u64);
        let track_solo = any_solo(&self.tracks);
        let bus_solo = any_solo(&self.buses);
        let tempo = TempoMap::new(self.transport.bpm(), self.sample_rate);
        for clock in &mut prepared.channel_clocks {
            let track = if clock.track.is_master() {
                Some(&self.master)
            } else {
                self.tracks
                    .iter()
                    .chain(&self.buses)
                    .find(|track| track.id == clock.track)
            };
            if let Some(track) = track {
                let position = if self.clip_performance {
                    if self.tracks.iter().any(|source| source.id == track.id) {
                        track
                            .active_clip
                            .map_or(0, |clip| clip.position.saturating_add(block.pos))
                    } else {
                        block.repeat_pos
                    }
                } else {
                    block.pos
                };
                clock.record(position, frames, !idle);
                for control in prepared
                    .automation_controls
                    .iter_mut()
                    .filter(|control| control.track == track.id)
                {
                    control.render(
                        &track.playback_source.automation,
                        position,
                        frames,
                        tempo.samples_per_beat(),
                        !idle,
                    );
                }
            }
        }
        for order_index in 0..prepared.graph.order.len() {
            let index = prepared.graph.order[order_index];
            let node = prepared.graph.nodes[index];
            prepared.nodes[index].samples[..len].fill(0.0);
            for (edge_index, edge) in prepared
                .graph
                .edges
                .iter()
                .enumerate()
                .filter(|(_, edge)| edge.to == index)
            {
                prepared.edge_samples[edge_index][..len]
                    .copy_from_slice(&prepared.nodes[edge.from].samples[..len]);
                if matches!(edge.kind, EdgeKind::Send(_)) {
                    let source = prepared.graph.nodes[edge.from].channel;
                    let track = self
                        .tracks
                        .iter()
                        .chain(&self.buses)
                        .find(|track| track.id == source);
                    let control = prepared.automation_controls.iter().find(|control| {
                        control.node == edge.from
                            && control.target
                                == vibez_core::automation::AutomationTarget::Send {
                                    bus_id: node.channel,
                                }
                    });
                    let clock = prepared
                        .channel_clocks
                        .iter()
                        .find(|clock| clock.track == source);
                    for frame in 0..frames {
                        let gain = track.map_or(0.0, |track| {
                            if let Some(control) = control {
                                let value = control.values[frame];
                                if value.is_nan() {
                                    track
                                        .sends
                                        .iter()
                                        .find(|(bus, _)| *bus == node.channel)
                                        .map_or(0.0, |(_, value)| *value)
                                } else {
                                    value
                                }
                            } else {
                                let position = clock.map_or(block.pos + frame as u64, |clock| {
                                    clock.position(
                                        prepared.compensation.node_input_latency[edge.from],
                                        frame,
                                    )
                                });
                                track.effective_send_amount(
                                    node.channel,
                                    position as f64 / tempo.samples_per_beat(),
                                )
                            }
                        });
                        for sample in &mut prepared.edge_samples[edge_index]
                            [frame * channels..(frame + 1) * channels]
                        {
                            *sample *= gain;
                        }
                    }
                }
                prepared.compensation.delay_edge(
                    edge_index,
                    &mut prepared.edge_samples[edge_index][..len],
                    channels,
                );
            }

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
                    if prepared
                        .automation_controls
                        .iter()
                        .any(|control| control.node == index)
                    {
                        track.apply_delayed_automation(index, &prepared.automation_controls, 0);
                    } else {
                        track.apply_node_automation(
                            pos as f64 / tempo.samples_per_beat(),
                            node.stage,
                        );
                    }
                    if let Some(instrument) = track.instrument.as_mut() {
                        instrument.set_audio_context(
                            vibez_core::audio_context::DeviceAudioContext {
                                musical_sample: pos,
                                continuous_sample,
                                sample_rate: self.sample_rate,
                                bpm: self.transport.bpm(),
                                playing: !idle,
                            },
                        );
                    }

                    let id = track.id;
                    let section = self.active_section;
                    let path = prepared
                        .compensation
                        .direct_path_latency(&prepared.graph, id);
                    let omitted = prepared.compensation.output_latency.saturating_sub(path);
                    let events = &mut self.event_tx;
                    let scheduled = &mut self.scheduled_presentation;
                    let presentation = &prepared.presentation;
                    let mut presentation_overflow = false;
                    let mut repeated = |trigger: crate::note_repeat::NoteRepeatTrigger| {
                        let context_for = |sample: u64| {
                            let offset = sample.saturating_sub(block.repeat_pos);
                            if offset < omitted as u64 {
                                presentation.before_block(omitted - offset as u32)
                            } else {
                                section.map(|active| {
                                    crate::compensation_clock::PresentationPosition {
                                        section_id: Some(active.section_id),
                                        section: Some(
                                            section_record::section_sample_for_performance(
                                                pos,
                                                block.repeat_pos,
                                                sample.saturating_sub(omitted as u64),
                                                active.length_samples,
                                            ),
                                        ),
                                        ..Default::default()
                                    }
                                })
                            }
                        };
                        let effective_context = context_for(trigger.effective_at_samples);
                        let canonical_context = context_for(trigger.canonical_at_samples);
                        let event = EngineEvent::NoteRepeated {
                            recording: crate::events::SourceRecordingPosition {
                                effective_at_samples: trigger.effective_at_samples,
                                canonical_at_samples: trigger.canonical_at_samples,
                                section_id: section.map(|active| active.section_id),
                                section_position_samples: section.map(|active| {
                                    section_record::section_sample_for_performance(
                                        pos,
                                        block.repeat_pos,
                                        trigger.effective_at_samples,
                                        active.length_samples,
                                    )
                                }),
                                canonical_section_position_samples: section.map(|active| {
                                    section_record::section_sample_for_performance(
                                        pos,
                                        block.repeat_pos,
                                        trigger.canonical_at_samples,
                                        active.length_samples,
                                    )
                                }),
                            },
                            track_id: id,
                            pitch: trigger.pitch,
                            velocity: trigger.velocity,
                            rate: trigger.rate,
                            effective_at_samples: trigger
                                .effective_at_samples
                                .saturating_sub(omitted as u64),
                            canonical_at_samples: trigger
                                .canonical_at_samples
                                .saturating_sub(omitted as u64),
                            section_id: effective_context.and_then(|context| context.section_id),
                            section_position_samples: effective_context
                                .and_then(|context| context.section),
                            canonical_section_position_samples: canonical_context
                                .and_then(|context| context.section),
                        };
                        if let EngineEvent::NoteRepeated { recording, .. } = &event {
                            let _ = events.push(EngineEvent::SourceNoteRepeated {
                                track_id: id,
                                pitch: trigger.pitch,
                                velocity: trigger.velocity,
                                rate: trigger.rate,
                                position: *recording,
                            });
                        }
                        if path == 0 {
                            let _ = events.push(event);
                        } else if scheduled.len() < scheduled.capacity() {
                            let due = continuous_sample
                                .saturating_add(
                                    trigger
                                        .effective_at_samples
                                        .saturating_sub(block.repeat_pos),
                                )
                                .saturating_add(path as u64);
                            let index = scheduled.partition_point(|pending| pending.due <= due);
                            scheduled
                                .insert(index, presentation::ScheduledPresentation { due, event });
                        } else {
                            presentation_overflow = true;
                            let _ = events.push(EngineEvent::CompensationInvalid {
                                track_id: id,
                                effect_id: None,
                                reason: "Audio presentation event history is full",
                            });
                        }
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
                    if presentation_overflow {
                        self.compensation_valid = false;
                        self.transport.stop();
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
                    if !node.channel.is_master()
                        && !self.buses.iter().any(|bus| bus.id == node.channel)
                    {
                        continue;
                    }
                    for (edge_index, edge) in prepared
                        .graph
                        .edges
                        .iter()
                        .enumerate()
                        .filter(|(_, edge)| edge.to == index)
                    {
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
                        for sample in 0..len {
                            prepared.nodes[index].samples[sample] +=
                                prepared.edge_samples[edge_index][sample];
                        }
                    }
                }
                NodeStage::Effect(effect_id) => {
                    for (edge_index, _) in prepared
                        .graph
                        .edges
                        .iter()
                        .enumerate()
                        .filter(|(_, edge)| edge.to == index && edge.kind == EdgeKind::Main)
                    {
                        for sample in 0..len {
                            prepared.nodes[index].samples[sample] +=
                                prepared.edge_samples[edge_index][sample];
                        }
                    }
                    let destination = &mut prepared.nodes[index];
                    for input in &mut destination.inputs {
                        if let Some((edge_index, _)) =
                            prepared.graph.edges.iter().enumerate().find(|(_, edge)| {
                                edge.to == index
                                    && edge.kind == EdgeKind::External(input.descriptor.id)
                            })
                        {
                            adapt_channels(
                                &prepared.edge_samples[edge_index][..len],
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
                        prepared.bypass_samples[index][..len]
                            .copy_from_slice(&destination.samples[..len]);
                        prepared.bypass_delays[index]
                            .process_layout(&mut prepared.bypass_samples[index][..len], channels);
                        let clock = prepared
                            .channel_clocks
                            .iter()
                            .find(|clock| clock.track == node.channel);
                        let delay = prepared.compensation.node_input_latency[index];
                        let controlled = prepared
                            .automation_controls
                            .iter()
                            .any(|control| control.node == index);
                        let mut offset = 0;
                        while offset < frames {
                            let target_pos = clock.map_or(block.pos + offset as u64, |clock| {
                                clock.position(delay, offset)
                            });
                            let valid = clock.is_none_or(|clock| clock.has_context(delay, offset));
                            let mut end = offset + 1;
                            while end < frames {
                                let same_context = clock.is_none_or(|clock| {
                                    clock.has_context(delay, end) == valid
                                        && (!valid
                                            || clock.position(delay, end)
                                                == target_pos + (end - offset) as u64)
                                });
                                let same_controls = prepared
                                    .automation_controls
                                    .iter()
                                    .filter(|control| control.node == index)
                                    .all(|control| {
                                        control.values[end].to_bits()
                                            == control.values[offset].to_bits()
                                    });
                                if !same_context || !same_controls {
                                    break;
                                }
                                end += 1;
                            }
                            if controlled {
                                track.apply_delayed_automation(
                                    index,
                                    &prepared.automation_controls,
                                    offset,
                                );
                            } else {
                                track.apply_node_automation(
                                    target_pos as f64 / tempo.samples_per_beat(),
                                    node.stage,
                                );
                            }
                            if let Some(slot) =
                                track.effects.iter_mut().find(|slot| slot.id == effect_id)
                            {
                                slot.effect.set_audio_context(
                                    vibez_core::audio_context::DeviceAudioContext {
                                        musical_sample: target_pos,
                                        continuous_sample: continuous_sample + offset as u64,
                                        sample_rate: self.sample_rate,
                                        bpm: self.transport.bpm(),
                                        playing: !idle,
                                    },
                                );
                                for (entry, input) in blocks.iter_mut().zip(&destination.inputs) {
                                    *entry = ExternalInputBlock {
                                        id: input.descriptor.id,
                                        channels: input.descriptor.channels,
                                        samples: &input.samples[offset * input.descriptor.channels
                                            ..end * input.descriptor.channels],
                                        connected: input.connected,
                                    };
                                }
                                slot.effect.process_with_inputs(
                                    &mut destination.samples[offset * channels..end * channels],
                                    channels,
                                    &blocks[..destination.inputs.len()],
                                );
                                if slot.bypass {
                                    destination.samples[offset * channels..end * channels]
                                        .copy_from_slice(
                                            &prepared.bypass_samples[index]
                                                [offset * channels..end * channels],
                                        );
                                }
                            }
                            offset = end;
                        }
                    }
                }
                NodeStage::AfterEffects | NodeStage::AfterFader => {
                    if let Some((edge_index, _)) = prepared
                        .graph
                        .edges
                        .iter()
                        .enumerate()
                        .find(|(_, edge)| edge.to == index && edge.kind == EdgeKind::Main)
                    {
                        for sample in 0..len {
                            prepared.nodes[index].samples[sample] =
                                prepared.edge_samples[edge_index][sample];
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
                            let target_pos = prepared
                                .channel_clocks
                                .iter()
                                .find(|clock| clock.track == node.channel)
                                .map_or(block.pos, |clock| {
                                    clock.position(
                                        prepared.compensation.node_input_latency[index],
                                        0,
                                    )
                                });
                            let delayed = prepared
                                .automation_controls
                                .iter()
                                .any(|control| control.node == index);
                            let (gain, pan) = if delayed {
                                track.apply_delayed_automation(
                                    index,
                                    &prepared.automation_controls,
                                    0,
                                )
                            } else {
                                track.apply_node_automation(
                                    target_pos as f64 / tempo.samples_per_beat(),
                                    node.stage,
                                )
                            };
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
                            if let Some(control) =
                                prepared.automation_controls.iter().find(|control| {
                                    control.node == index
                                        && control.target
                                            == vibez_core::automation::AutomationTarget::TrackMute
                                })
                            {
                                track.apply_delayed_mute_envelope(
                                    &control.values,
                                    frames,
                                    channels,
                                );
                            } else {
                                track.apply_mute_envelope(
                                    target_pos,
                                    frames,
                                    channels,
                                    tempo.samples_per_beat(),
                                );
                            }
                            for frame in 0..frames {
                                let (gain, left, right) = if delayed {
                                    let (frame_gain, frame_pan) = track.apply_delayed_automation(
                                        index,
                                        &prepared.automation_controls,
                                        frame,
                                    );
                                    let (left, right) = if bus_channel {
                                        crate::mixer::balance_pan(frame_pan.unwrap_or(track.pan))
                                    } else {
                                        equal_power_pan(frame_pan.unwrap_or(track.pan))
                                    };
                                    (frame_gain.unwrap_or(track.gain), left, right)
                                } else {
                                    (gain, left, right)
                                };
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
        {
            let performing = self.clock_domain == ClockDomain::Perform;
            let first = crate::compensation_clock::PresentationPosition {
                arrange: if performing {
                    self.transport.position()
                } else {
                    block.pos
                },
                perform: block.repeat_pos,
                section: self.active_section.map(|_| block.pos),
                section_id: self.active_section.map(|section| section.section_id),
                section_length: self
                    .active_section
                    .map_or(0, |section| section.length_samples),
                generation: prepared.compensation.generation,
            };
            prepared.presentation_start.get_or_insert(first);
            prepared
                .presentation
                .record_clocks(first, frames, !performing && !idle, !idle);
        }
        self.rendered_callback_frames += frames;
        self.routing = Some(prepared);
    }
}
