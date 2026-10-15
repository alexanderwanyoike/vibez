//! Owns retained timing dependencies and bounded per-line Crossfade/Glide movement.

use vibez_core::constants::{DEFAULT_BPM, MAX_BPM, MIN_BPM};

use super::{history::ReadBlend, param};

#[derive(Clone, Copy)]
struct Crossfade {
    old: f64,
    destination: f64,
    frame: usize,
}

#[derive(Clone, Copy)]
struct Glide {
    origin: f64,
    destination: f64,
    frames: usize,
    frame: usize,
}

#[derive(Clone, Copy)]
enum Movement {
    Idle,
    Crossfade(Crossfade),
    Glide(Glide),
}

#[derive(Clone, Copy)]
struct LineTime {
    current: f64,
    movement: Movement,
    pending: Option<f64>,
}

impl LineTime {
    fn new() -> Self {
        Self {
            current: 0.0,
            movement: Movement::Idle,
            pending: None,
        }
    }

    fn snap(&mut self, target: f64) {
        self.current = target;
        self.movement = Movement::Idle;
        self.pending = None;
    }

    fn request(&mut self, target: f64, use_glide: bool, sample_rate: f32) {
        if let Movement::Crossfade(fade) = self.movement {
            self.pending = (target != fade.destination).then_some(target);
            return;
        }
        self.pending = None;
        self.movement = if target == self.current {
            Movement::Idle
        } else if use_glide {
            let frames = (0.05 * f64::from(sample_rate))
                .max(2.0 * (target - self.current).abs())
                .ceil() as usize;
            Movement::Glide(Glide {
                origin: self.current,
                destination: target,
                frames,
                frame: 0,
            })
        } else {
            Movement::Crossfade(Crossfade {
                old: self.current,
                destination: target,
                frame: 0,
            })
        };
    }

    fn next_read(&mut self, fade_frames: usize, sample_rate: f32, use_glide: bool) -> ReadBlend {
        if let Movement::Glide(mut glide) = self.movement {
            glide.frame += 1;
            // Origin-based progress avoids accumulated drift while preserving the exact final target.
            self.current = if glide.frame == glide.frames {
                glide.destination
            } else {
                glide.origin
                    + (glide.destination - glide.origin) * glide.frame as f64 / glide.frames as f64
            };
            self.movement = if glide.frame == glide.frames {
                Movement::Idle
            } else {
                Movement::Glide(glide)
            };
            return ReadBlend {
                old: self.current,
                destination: self.current,
                progress: 0.0,
            };
        }
        let Movement::Crossfade(mut fade) = self.movement else {
            return ReadBlend {
                old: self.current,
                destination: self.current,
                progress: 0.0,
            };
        };
        // Both endpoints must fit within N frames to preserve the literal two-fade settle bound.
        let read = ReadBlend {
            old: fade.old,
            destination: fade.destination,
            progress: fade.frame as f64 / (fade_frames - 1) as f64,
        };
        fade.frame += 1;
        if fade.frame == fade_frames {
            self.current = fade.destination;
            self.movement = Movement::Idle;
            if let Some(target) = self.pending.take() {
                self.request(target, use_glide, sample_rate);
            }
        } else {
            self.movement = Movement::Crossfade(fade);
        }
        read
    }
}

pub(super) struct DelayTime {
    bpm: f64,
    started: bool,
    lines: [LineTime; 2],
}

impl DelayTime {
    pub(super) fn new() -> Self {
        Self {
            bpm: DEFAULT_BPM,
            started: false,
            lines: [LineTime::new(); 2],
        }
    }

    pub(super) fn set_bpm(&mut self, bpm: f64, targets: &[f32; param::COUNT], sample_rate: f32) {
        if !bpm.is_finite() {
            return;
        }
        let bpm = bpm.clamp(MIN_BPM, MAX_BPM);
        if bpm == self.bpm {
            return;
        }
        self.bpm = bpm;
        if self.started && targets[param::SYNC] >= 0.5 {
            self.snap(targets, sample_rate);
        }
    }

    pub(super) fn begin_audio(&mut self, targets: &[f32; param::COUNT], sample_rate: f32) {
        if !self.started {
            self.snap(targets, sample_rate);
            self.started = true;
        }
    }

    pub(super) fn parameter_edited(
        &mut self,
        index: usize,
        targets: &[f32; param::COUNT],
        sample_rate: f32,
        automated: bool,
    ) {
        if !self.started {
            return;
        }
        if index == param::TIME_CHANGE && automated {
            self.snap(targets, sample_rate);
            return;
        }
        let sync = targets[param::SYNC] >= 0.5;
        let linked = targets[param::LINK] >= 0.5;
        let left = match index {
            param::SYNC | param::TIME_CHANGE => true,
            param::TIME_MS => !sync,
            param::LEFT_DIVISION | param::LEFT_MODIFIER => sync,
            _ => false,
        };
        let right = match index {
            param::SYNC | param::LINK | param::TIME_CHANGE => true,
            param::TIME_MS => linked && !sync,
            param::LEFT_DIVISION | param::LEFT_MODIFIER => linked && sync,
            param::RIGHT_TIME_MS => !linked && !sync,
            param::RIGHT_DIVISION | param::RIGHT_MODIFIER => !linked && sync,
            _ => false,
        };
        if !left && !right {
            return;
        }
        let distances = self.distances(targets, sample_rate);
        let use_glide = targets[param::TIME_CHANGE] >= 0.5;
        for ((line, target), affected) in self.lines.iter_mut().zip(distances).zip([left, right]) {
            if affected {
                if automated {
                    line.snap(target);
                } else {
                    line.request(target, use_glide, sample_rate);
                }
            }
        }
    }

    fn snap(&mut self, targets: &[f32; param::COUNT], sample_rate: f32) {
        let distances = self.distances(targets, sample_rate);
        for (line, target) in self.lines.iter_mut().zip(distances) {
            line.snap(target);
        }
    }

    pub(super) fn next_frame(
        &mut self,
        fade_frames: usize,
        sample_rate: f32,
        use_glide: bool,
    ) -> [ReadBlend; 2] {
        self.lines
            .each_mut()
            .map(|line| line.next_read(fade_frames, sample_rate, use_glide))
    }

    pub(super) fn snapshot(
        &self,
        targets: &[f32; param::COUNT],
        sample_rate: f32,
    ) -> (f64, [vibez_core::delay_state::DelayLineState; 2]) {
        use vibez_core::delay_state::DelayLineState;
        let fade_frames = (sample_rate as usize).div_ceil(50);
        let distances = self.distances(targets, sample_rate);
        let lines = std::array::from_fn(|index| {
            let line = self.lines[index];
            let mut state = DelayLineState {
                old_samples: line.current,
                destination_samples: line.current,
                progress: 0.0,
                target_samples: distances[index],
                pending_samples: line.pending,
                remaining_frames: 0,
                moving: false,
                gliding: false,
            };
            match line.movement {
                Movement::Idle => {}
                Movement::Crossfade(fade) => {
                    state.old_samples = fade.old;
                    state.destination_samples = fade.destination;
                    // The cursor already points at the next frame; observers see the last audible blend.
                    state.progress = fade.frame.saturating_sub(1) as f64 / (fade_frames - 1) as f64;
                    state.remaining_frames = (fade_frames - fade.frame) as u64;
                    if let Some(pending) = line.pending {
                        let next = if targets[param::TIME_CHANGE] >= 0.5 {
                            (0.05 * f64::from(sample_rate))
                                .max(2.0 * (pending - fade.destination).abs())
                                .ceil() as u64
                        } else {
                            fade_frames as u64
                        };
                        state.remaining_frames = state.remaining_frames.saturating_add(next);
                    }
                    state.moving = true;
                }
                Movement::Glide(glide) => {
                    state.remaining_frames = (glide.frames - glide.frame) as u64;
                    state.moving = true;
                    state.gliding = true;
                }
            }
            state
        });
        (self.bpm, lines)
    }

    pub(super) fn reset(&mut self, targets: &[f32; param::COUNT], sample_rate: f32) {
        self.snap(targets, sample_rate);
        self.started = false;
    }

    fn distances(&self, targets: &[f32; param::COUNT], sample_rate: f32) -> [f64; 2] {
        let left = self.distance(
            targets,
            sample_rate,
            param::TIME_MS,
            param::LEFT_DIVISION,
            param::LEFT_MODIFIER,
        );
        let right = if targets[param::LINK] >= 0.5 {
            left
        } else {
            self.distance(
                targets,
                sample_rate,
                param::RIGHT_TIME_MS,
                param::RIGHT_DIVISION,
                param::RIGHT_MODIFIER,
            )
        };
        [left, right]
    }

    fn distance(
        &self,
        targets: &[f32; param::COUNT],
        sample_rate: f32,
        time: usize,
        division: usize,
        modifier: usize,
    ) -> f64 {
        if targets[param::SYNC] >= 0.5 {
            let denominator = [64.0, 32.0, 16.0, 8.0, 4.0, 2.0, 1.0][targets[division] as usize];
            let modifier = [1.0, 1.5, 2.0 / 3.0][targets[modifier] as usize];
            (60.0 / self.bpm) * (4.0 / denominator) * modifier * f64::from(sample_rate)
        } else {
            f64::from(targets[time]) * f64::from(sample_rate) / 1000.0
        }
    }
}
