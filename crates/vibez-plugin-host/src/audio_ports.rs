//! Shared planar port preparation and mono/stereo delivery laws.

use vibez_core::routing::{ExternalInputBlock, ExternalInputDescriptor, ExternalInputId};

pub(crate) struct AudioPort {
    pub id: ExternalInputId,
    pub channels: usize,
    pub main: bool,
    pub available: bool,
    pub samples: Vec<Vec<f32>>,
    pub pointers: Vec<*mut f32>,
}

impl AudioPort {
    pub fn new(id: ExternalInputId, channels: usize, main: bool, max_frames: usize) -> Self {
        let mut samples: Vec<Vec<f32>> = (0..channels).map(|_| vec![0.0; max_frames]).collect();
        let pointers = samples
            .iter_mut()
            .map(|channel| channel.as_mut_ptr())
            .collect();
        Self {
            id,
            channels,
            main,
            available: true,
            samples,
            pointers,
        }
    }

    pub fn fill_input(
        &mut self,
        main: &[f32],
        main_channels: usize,
        inputs: &[ExternalInputBlock<'_>],
        frames: usize,
    ) {
        let input = inputs
            .iter()
            .find(|input| input.id == self.id && input.connected);
        let (samples, channels) = if self.main {
            (main, main_channels)
        } else {
            input.map_or((&[][..], self.channels), |input| {
                (input.samples, input.channels)
            })
        };
        for channel in 0..self.channels {
            for frame in 0..frames {
                self.samples[channel][frame] = if self.available {
                    vibez_core::routing::adapt_channel_sample(
                        channels,
                        self.channels,
                        channel,
                        |source_channel| {
                            samples
                                .get(frame * channels + source_channel)
                                .copied()
                                .unwrap_or(0.0)
                        },
                    )
                } else {
                    0.0
                };
            }
        }
    }

    pub fn copy_output(&self, output: &mut [f32], output_channels: usize, frames: usize) {
        for frame in 0..frames {
            for channel in 0..output_channels {
                output[frame * output_channels + channel] =
                    vibez_core::routing::adapt_channel_sample(
                        self.channels,
                        output_channels,
                        channel,
                        |source_channel| self.samples[source_channel][frame],
                    );
            }
        }
    }
}

pub(crate) fn descriptors(ports: &[(String, AudioPort)]) -> Vec<ExternalInputDescriptor> {
    ports
        .iter()
        .filter(|(_, port)| !port.main && port.available && matches!(port.channels, 1 | 2))
        .map(|(name, port)| ExternalInputDescriptor {
            id: port.id,
            name: name.clone(),
            channels: port.channels,
        })
        .collect()
}

pub(crate) fn prepare_process_ports(
    input_ports: &mut [(String, AudioPort)],
    output_ports: &mut [(String, AudioPort)],
    main: &[f32],
    main_channels: usize,
    inputs: &[ExternalInputBlock<'_>],
    frames: usize,
) {
    for (_, port) in input_ports {
        port.fill_input(main, main_channels, inputs, frames);
    }
    for (_, port) in output_ports {
        for channel in &mut port.samples {
            channel[..frames].fill(0.0);
        }
    }
}
pub(crate) fn copy_process_output(
    ports: &[(String, AudioPort)],
    output: &mut [f32],
    channels: usize,
    frames: usize,
) {
    if let Some((_, port)) = ports
        .iter()
        .find(|(_, port)| port.main && port.available)
        .or_else(|| ports.iter().find(|(_, port)| port.available))
    {
        port.copy_output(output, channels, frames);
    } else {
        output.fill(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planar_and_interleaved_conversion_share_the_same_law() {
        for source_channels in [1, 2, 6] {
            for destination_channels in [1, 2, 6] {
                let source: Vec<_> = (0..source_channels * 3).map(|value| value as f32).collect();
                let mut expected = vec![0.0; destination_channels * 3];
                vibez_core::routing::adapt_channels(
                    &source,
                    source_channels,
                    &mut expected,
                    destination_channels,
                );
                let mut port = AudioPort::new(ExternalInputId(0), destination_channels, true, 3);
                port.fill_input(&source, source_channels, &[], 3);
                for frame in 0..3 {
                    for channel in 0..destination_channels {
                        assert_eq!(
                            port.samples[channel][frame],
                            expected[frame * destination_channels + channel]
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn unavailable_ports_are_silent_and_outputless_devices_do_not_pass_dry_audio() {
        let mut port = AudioPort::new(ExternalInputId(0), 2, false, 3);
        port.available = false;
        port.fill_input(
            &[],
            2,
            &[ExternalInputBlock {
                id: port.id,
                channels: 2,
                samples: &[1.0; 6],
                connected: true,
            }],
            3,
        );
        assert!(port.samples.iter().flatten().all(|sample| *sample == 0.0));
        let mut output = [1.0; 6];
        copy_process_output(&[], &mut output, 2, 3);
        assert_eq!(output, [0.0; 6]);
    }
}
