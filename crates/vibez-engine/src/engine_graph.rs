//! Execute prepared stage buffers without callback graph searches or allocation.

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
        if self.project_processing_muted() {
            output.fill(0.0);
            if let Some(capture) = capture.as_deref_mut() {
                capture.samples.fill(0.0);
            }
            return;
        }
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
                let mut part_capture = capture.as_deref_mut().and_then(|capture| {
                    let source_track_raw = capture.source_track_raw;
                    capture
                        .samples
                        .get_mut(start..end)
                        .map(|samples| TrackOutputCapture {
                            source_track_raw,
                            samples,
                        })
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
        if block.channels > 2 {
            self.render_hardware_routing(output, block, capture, idle, capture_audible_only);
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
        for channel in &mut prepared.channels {
            channel.bind(&self.tracks, &self.buses);
        }
        for channel_index in 0..prepared.channels.len() {
            let binding = prepared.channels[channel_index].binding;
            let clock = &mut prepared.channel_clocks[channel_index];
            if let Some(track) = binding.get(&self.tracks, &self.buses, &self.master) {
                let position = if self.clip_performance {
                    if matches!(binding, crate::routing::ChannelIndex::Track(_)) {
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
        prepared.select_audible_returns(
            &self.tracks,
            &self.buses,
            &self.master,
            track_solo,
            bus_solo,
        );
        for order_index in 0..prepared.graph.order.len() {
            let index = prepared.graph.order[order_index];
            let node = prepared.graph.nodes[index];
            let channel_index = prepared.nodes[index].channel;
            let binding = prepared.channels[channel_index].binding;
            prepared.nodes[index].samples[..len].fill(0.0);
            for incoming_index in 0..prepared.nodes[index].incoming.len() {
                let edge_index = prepared.nodes[index].incoming[incoming_index];
                let edge = prepared.graph.edges[edge_index];
                prepared.edge_samples[edge_index][..len]
                    .copy_from_slice(&prepared.nodes[edge.from].samples[..len]);
                if matches!(edge.kind, EdgeKind::Send) {
                    let source_channel = prepared.nodes[edge.from].channel;
                    let track = prepared.channels[source_channel].binding.get(
                        &self.tracks,
                        &self.buses,
                        &self.master,
                    );
                    let control = prepared.automation_controls
                        [prepared.nodes[edge.from].controls.clone()]
                    .iter()
                    .find(|control| {
                        control.target
                            == vibez_core::automation::AutomationTarget::Send {
                                bus_id: node.channel,
                            }
                    });
                    let clock = Some(&prepared.channel_clocks[source_channel]);
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
                    if self.is_device_recovering(node.channel, None) {
                        continue;
                    }
                    let Some(track) =
                        binding.get_mut(&mut self.tracks, &mut self.buses, &mut self.master)
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
                    if !prepared.nodes[index].controls.is_empty() {
                        track.apply_delayed_automation(
                            &prepared.automation_controls[prepared.nodes[index].controls.clone()],
                            0,
                        );
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
                    let path = prepared.channels[channel_index].direct_latency;
                    let omitted = prepared.compensation.output_latency.saturating_sub(path);
                    let events = &mut self.event_tx;
                    let scheduled = &mut self.scheduled_presentation;
                    let presentation = &prepared.presentation;
                    let performing = self.clock_domain == ClockDomain::Perform;
                    let mut presentation_overflow = false;
                    let mut repeated = |trigger: crate::note_repeat::NoteRepeatTrigger| {
                        if presentation_overflow {
                            return;
                        }
                        let context_for = |sample: u64| {
                            let relative =
                                sample as i128 - block.repeat_pos as i128 - omitted as i128;
                            if relative < 0 {
                                u32::try_from(-relative)
                                    .ok()
                                    .and_then(|delay| presentation.before_block(delay))
                            } else {
                                let delta = relative as u64;
                                Some(crate::compensation_clock::PresentationPosition {
                                    arrange: block.pos.saturating_add(delta),
                                    perform: block.repeat_pos.saturating_add(delta),
                                    section_id: section.map(|active| active.section_id),
                                    section: section.map(|active| {
                                        section_record::section_sample_for_performance(
                                            pos,
                                            block.repeat_pos,
                                            sample.saturating_sub(omitted as u64),
                                            active.length_samples,
                                        )
                                    }),
                                    ..Default::default()
                                })
                            }
                        };
                        let effective_context = context_for(trigger.effective_at_samples);
                        let canonical_context = context_for(trigger.canonical_at_samples);
                        let recording = crate::events::SourceRecordingPosition {
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
                        };
                        let event = EngineEvent::NoteRepeated {
                            track_id: id,
                            pitch: trigger.pitch,
                            velocity: trigger.velocity,
                            rate: trigger.rate,
                            effective_at_samples: effective_context.map_or_else(
                                || trigger.effective_at_samples.saturating_sub(omitted as u64),
                                |context| {
                                    if performing {
                                        context.perform
                                    } else {
                                        context.arrange
                                    }
                                },
                            ),
                            canonical_at_samples: canonical_context.map_or_else(
                                || trigger.canonical_at_samples.saturating_sub(omitted as u64),
                                |context| {
                                    if performing {
                                        context.perform
                                    } else {
                                        context.arrange
                                    }
                                },
                            ),
                            section_id: effective_context.and_then(|context| context.section_id),
                            section_position_samples: effective_context
                                .and_then(|context| context.section),
                            canonical_section_position_samples: canonical_context
                                .and_then(|context| context.section),
                        };
                        let source = EngineEvent::SourceNoteRepeated {
                            track_id: id,
                            pitch: trigger.pitch,
                            velocity: trigger.velocity,
                            rate: trigger.rate,
                            position: recording,
                        };
                        let due = continuous_sample
                            .saturating_add(
                                trigger
                                    .effective_at_samples
                                    .saturating_sub(block.repeat_pos),
                            )
                            .saturating_add(path as u64);
                        presentation_overflow = !presentation_queue::emit_repeated(
                            source,
                            event,
                            events,
                            scheduled,
                            continuous_sample,
                            due,
                        );
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
                    if presentation_overflow {
                        self.fail_presentation();
                    }
                    if activity != 0 {
                        let _ = self.event_tx.push(EngineEvent::TrackNoteActivity {
                            track_id: id,
                            triggered_notes: activity,
                        });
                    }
                }
                NodeStage::Sum => {
                    if binding
                        .get(&self.tracks, &self.buses, &self.master)
                        .is_none()
                    {
                        continue;
                    }
                    for incoming_index in 0..prepared.nodes[index].incoming.len() {
                        let edge_index = prepared.nodes[index].incoming[incoming_index];
                        let edge = prepared.graph.edges[edge_index];
                        let source_channel = &prepared.channels[prepared.nodes[edge.from].channel];
                        let target_channel = &prepared.channels[prepared.nodes[index].channel];
                        let audible = match edge.kind {
                            EdgeKind::Mix => source_channel.audible_return,
                            EdgeKind::Send => {
                                !track_solo
                                    || bus_solo
                                    || source_channel.audible_return
                                    || (!target_channel.audible_return
                                        && prepared.detector_buses.contains(&node.channel))
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
                    let recovery_bypass = self.recovering_effect_bypass(node.channel, effect_id);
                    for incoming_index in 0..prepared.nodes[index].incoming.len() {
                        let edge_index = prepared.nodes[index].incoming[incoming_index];
                        if prepared.graph.edges[edge_index].kind != EdgeKind::Main {
                            continue;
                        }
                        for sample in 0..len {
                            prepared.nodes[index].samples[sample] +=
                                prepared.edge_samples[edge_index][sample];
                        }
                    }
                    let destination = &mut prepared.nodes[index];
                    for input in &mut destination.inputs {
                        if let Some((edge_index, _)) = destination
                            .incoming
                            .iter()
                            .map(|&edge_index| (edge_index, &prepared.graph.edges[edge_index]))
                            .find(|(_, edge)| edge.kind == EdgeKind::External(input.descriptor.id))
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
                    let track =
                        binding.get_mut(&mut self.tracks, &mut self.buses, &mut self.master);
                    if let Some(track) = track {
                        prepared.bypass_samples[index][..len]
                            .copy_from_slice(&destination.samples[..len]);
                        prepared.bypass_delays[index]
                            .process_layout(&mut prepared.bypass_samples[index][..len], channels);
                        if let Some(bypass) = recovery_bypass {
                            if bypass {
                                destination.samples[..len]
                                    .copy_from_slice(&prepared.bypass_samples[index][..len]);
                            } else {
                                destination.samples[..len].fill(0.0);
                            }
                            continue;
                        }
                        let clock = Some(&prepared.channel_clocks[channel_index]);
                        let delay = prepared.compensation.node_input_latency[index];
                        let controls = &prepared.automation_controls[destination.controls.clone()];
                        let controlled = !controls.is_empty();
                        let mut offset = 0;
                        while offset < frames {
                            let target_pos = clock.map_or(block.pos + offset as u64, |clock| {
                                clock.position(delay, offset)
                            });
                            let limit = if controlled {
                                frames.min(offset + automation_interval(self.sample_rate))
                            } else {
                                frames
                            };
                            let end = clock
                                .map_or(limit, |clock| clock.segment_end(delay, offset, limit));
                            if controlled {
                                track.apply_delayed_automation(controls, offset);
                            } else {
                                track.apply_node_automation(
                                    target_pos as f64 / tempo.samples_per_beat(),
                                    node.stage,
                                );
                            }
                            if let Some(slot) =
                                track.effects.iter_mut().find(|slot| slot.id == effect_id)
                            {
                                if destination
                                    .bypass_state
                                    .is_some_and(|previous| previous != slot.bypass)
                                    && !slot.bypass
                                {
                                    // Stopped wet DSP must not replay input from before bypass.
                                    slot.effect.reset();
                                    destination.wet_warmup = prepared.device_latencies[index];
                                    destination.wet_fade_in = 64;
                                }
                                destination.bypass_state = Some(slot.bypass);
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
                                if !slot.bypass {
                                    slot.effect.process_with_inputs(
                                        &mut destination.samples[offset * channels..end * channels],
                                        channels,
                                        &blocks[..destination.inputs.len()],
                                    );
                                    for frame in offset..end {
                                        let blend = if destination.wet_warmup > 0 {
                                            destination.wet_warmup -= 1;
                                            0.0
                                        } else if destination.wet_fade_in > 0 {
                                            let blend = 1.0 - destination.wet_fade_in as f32 / 64.0;
                                            destination.wet_fade_in -= 1;
                                            blend
                                        } else {
                                            1.0
                                        };
                                        if blend < 1.0 {
                                            for channel in 0..channels {
                                                let sample = frame * channels + channel;
                                                destination.samples[sample] = prepared
                                                    .bypass_samples[index][sample]
                                                    * (1.0 - blend)
                                                    + destination.samples[sample] * blend;
                                            }
                                        }
                                    }
                                } else {
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
                    if let Some(&edge_index) =
                        prepared.nodes[index].incoming.iter().find(|&&edge_index| {
                            prepared.graph.edges[edge_index].kind == EdgeKind::Main
                        })
                    {
                        for sample in 0..len {
                            prepared.nodes[index].samples[sample] =
                                prepared.edge_samples[edge_index][sample];
                        }
                    }
                    let bus_channel = !matches!(binding, crate::routing::ChannelIndex::Track(_));
                    let track =
                        binding.get_mut(&mut self.tracks, &mut self.buses, &mut self.master);
                    if let Some(track) = track {
                        if node.stage == NodeStage::AfterFader {
                            let target_pos = prepared.channel_clocks[channel_index]
                                .position(prepared.compensation.node_input_latency[index], 0);
                            let controls = &prepared.automation_controls
                                [prepared.nodes[index].controls.clone()];
                            let delayed = !controls.is_empty();
                            let (gain, pan) = if delayed {
                                track.apply_delayed_automation(controls, 0)
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
                            if let Some(control) = controls.iter().find(|control| {
                                control.target
                                    == vibez_core::automation::AutomationTarget::TrackMute
                            }) {
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
                                    let (frame_gain, frame_pan) =
                                        track.delayed_mix_values(controls, frame);
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
                            if let Some(capture) = capture.as_deref_mut().filter(|capture| {
                                capture.source_track_raw == node.channel.raw()
                                    && capture.samples.len() >= len
                            }) {
                                let audible =
                                    prepared.channels[prepared.nodes[index].channel].audible_return;
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

const MAX_AUTOMATION_INTERVAL: usize = 64;

fn automation_interval(sample_rate: u32) -> usize {
    (u64::from(sample_rate) * MAX_AUTOMATION_INTERVAL as u64)
        .div_ceil(48_000)
        .clamp(1, MAX_AUTOMATION_INTERVAL as u64) as usize
}
