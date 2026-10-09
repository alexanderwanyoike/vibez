use vibez_core::routing::{ExternalInputBlock, ExternalInputDescriptor, ExternalInputId};

pub(crate) struct AudioPort {
    pub id: ExternalInputId,
    pub channels: usize,
    pub main: bool,
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
                self.samples[channel][frame] = if samples.is_empty() {
                    0.0
                } else if self.channels == 1 && channels == 2 {
                    (samples[frame * 2] + samples[frame * 2 + 1]) * 0.5
                } else {
                    samples
                        .get(frame * channels + channel.min(channels - 1))
                        .copied()
                        .unwrap_or(0.0)
                };
            }
        }
    }

    pub fn copy_output(&self, output: &mut [f32], output_channels: usize, frames: usize) {
        for frame in 0..frames {
            for channel in 0..output_channels {
                output[frame * output_channels + channel] = if self.channels == 0 {
                    0.0
                } else if output_channels == 1 && self.channels == 2 {
                    (self.samples[0][frame] + self.samples[1][frame]) * 0.5
                } else {
                    self.samples[channel.min(self.channels - 1)][frame]
                };
            }
        }
    }
}

pub(crate) fn descriptors(ports: &[(String, AudioPort)]) -> Vec<ExternalInputDescriptor> {
    ports
        .iter()
        .filter(|(_, port)| !port.main)
        .map(|(name, port)| ExternalInputDescriptor {
            id: port.id,
            name: name.clone(),
            channels: port.channels,
        })
        .collect()
}
