pub struct Timing {
    history: [f32; 8192],
    cursor: usize,
    actual: usize,
    reported: u32,
    probe: bool,
    context_mode: bool,
    context: u64,
    pending: [u32; 3],
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            history: [0.0; 8192],
            cursor: 0,
            actual: 0,
            reported: 0,
            probe: true,
            context_mode: false,
            context: 0,
            pending: [0, 0, 1],
        }
    }
}

impl Timing {
    pub fn configure(&mut self, bytes: &[u8; 12]) -> bool {
        let values = std::array::from_fn(|index| {
            u32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap())
        });
        if values[0] > 4096 {
            return false;
        }
        self.pending = values;
        true
    }

    pub fn state(&self) -> [u8; 12] {
        let mut bytes = [0; 12];
        for (index, value) in self.pending.iter().enumerate() {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    pub fn activation_allowed(&self) -> bool {
        self.pending[2] & (1 << 17) == 0
    }
    pub fn activate(&mut self) {
        self.actual = self.pending[0] as usize;
        self.reported = self.pending[1];
        self.probe = self.pending[2] & 0xffff == 1;
        self.context_mode = self.pending[2] & 0xffff == 2;
        self.reset();
    }

    pub fn reset(&mut self) {
        self.history.fill(0.0);
        self.cursor = 0;
    }
    pub fn report(&self) -> u32 {
        self.reported
    }

    pub fn mono_channels(&self) -> usize {
        if self.pending[2] & (1 << 16) != 0 {
            2
        } else {
            1
        }
    }

    pub fn set_context(&mut self, context: u64) {
        self.context = context;
    }

    pub fn frame(&mut self, main: [f32; 2], mono: f32, stereo: [f32; 2]) -> [f32; 2] {
        let input = if self.context_mode {
            let values = [self.context as f32; 2];
            self.context += 1;
            values
        } else if self.probe {
            [
                main[0] + 2.0 * mono + 3.0 * stereo[0],
                main[1] + 2.0 * mono + 3.0 * stereo[1],
            ]
        } else {
            main
        };
        if self.actual == 0 {
            return input;
        }
        let result = [self.history[self.cursor], self.history[self.cursor + 1]];
        self.history[self.cursor..self.cursor + 2].copy_from_slice(&input);
        self.cursor = (self.cursor + 2) % (self.actual * 2);
        result
    }
}
