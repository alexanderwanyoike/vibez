//! Owns retained timing dependencies and bounded per-line fixed-tap manual Crossfades.

use vibez_core::constants::{DEFAULT_BPM, MAX_BPM, MIN_BPM};

use super::{history::ReadBlend, param};

#[derive(Clone, Copy)]
struct Crossfade {
    old: f64,
    destination: f64,
    frame: usize,
}

#[derive(Clone, Copy)]
struct LineTime {
    current: f64,
    fade: Option<Crossfade>,
    pending: Option<f64>,
}

impl LineTime {
    fn new() -> Self {
        Self { current: 0.0, fade: None, pending: None }
    }

    fn snap(&mut self, target: f64) {
        self.current = target;
        self.fade = None;
        self.pending = None;
    }

    fn request(&mut self, target: f64) {
        if let Some(fade) = self.fade {
            self.pending = (target != fade.destination).then_some(target);
        } else if target != self.current {
            self.fade = Some(Crossfade { old: self.current, destination: target, frame: 0 });
        }
    }

    fn next_read(&mut self, frames: usize) -> ReadBlend {
        let Some(mut fade) = self.fade else {
            return ReadBlend { old: self.current, destination: self.current, progress: 0.0 };
        };
        // Both endpoints must fit within N frames to preserve the literal two-fade settle bound.
        let read = ReadBlend {
            old: fade.old,
            destination: fade.destination,
            progress: fade.frame as f64 / (frames - 1) as f64,
        };
        fade.frame += 1;
        if fade.frame == frames {
            self.current = fade.destination;
            self.fade = None;
            if let Some(target) = self.pending.take() {
                self.request(target);
            }
        } else {
            self.fade = Some(fade);
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
        Self { bpm: DEFAULT_BPM, started: false, lines: [LineTime::new(); 2] }
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
            param::SYNC => true,
            param::TIME_MS => !sync,
            param::LEFT_DIVISION | param::LEFT_MODIFIER => sync,
            _ => false,
        };
        let right = match index {
            param::SYNC | param::LINK => true,
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
        for ((line, target), affected) in self.lines.iter_mut().zip(distances).zip([left, right]) {
            if affected {
                if automated { line.snap(target); } else { line.request(target); }
            }
        }
    }

    fn snap(&mut self, targets: &[f32; param::COUNT], sample_rate: f32) {
        let distances = self.distances(targets, sample_rate);
        for (line, target) in self.lines.iter_mut().zip(distances) {
            line.snap(target);
        }
    }

    pub(super) fn next_frame(&mut self, fade_frames: usize) -> [ReadBlend; 2] {
        self.lines.each_mut().map(|line| line.next_read(fade_frames))
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
            let denominator = [64.0, 32.0, 16.0, 8.0, 4.0, 2.0, 1.0]
                [targets[division] as usize];
            let modifier = [1.0, 1.5, 2.0 / 3.0][targets[modifier] as usize];
            (60.0 / self.bpm) * (4.0 / denominator) * modifier * f64::from(sample_rate)
        } else {
            f64::from(targets[time]) * f64::from(sample_rate) / 1000.0
        }
    }
}
