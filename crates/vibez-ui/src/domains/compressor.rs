//! Owns native Compressor panel choices and bounded measured dynamics history.

use crate::state::UiEffect;
use std::time::{Duration, Instant};
use vibez_core::{
    effect::{DynamicsMeter, EffectType},
    id::{EffectId, TrackId},
    routing::ExternalInputId,
};

pub const HISTORY_WINDOWS: usize = 120;

pub fn is_native_compressor(effect: &UiEffect) -> bool {
    effect.effect_type == EffectType::Compressor
        && effect.plugin_ref.is_none()
        && effect.plugin_name.is_none()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CompressorPage {
    #[default]
    Sound,
    Dynamics,
    Detector,
    Range,
    Sidechain,
    Metering,
}
impl CompressorPage {
    pub const ALL: [Self; 6] = [
        Self::Sound,
        Self::Dynamics,
        Self::Detector,
        Self::Range,
        Self::Sidechain,
        Self::Metering,
    ];
}
impl std::fmt::Display for CompressorPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Sound => "Sound",
            Self::Dynamics => "Dynamics",
            Self::Detector => "Detector",
            Self::Range => "Range",
            Self::Sidechain => "Sidechain",
            Self::Metering => "Metering",
        })
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CompressorVisual {
    #[default]
    Transfer,
    History,
}

#[derive(Clone, Debug)]
pub enum CompressorMsg {
    Page(TrackId, EffectId, CompressorPage),
    Visual(TrackId, EffectId, CompressorVisual),
    SidechainInput(TrackId, EffectId, ExternalInputId),
}
impl CompressorMsg {
    pub fn destination(&self) -> (TrackId, EffectId) {
        match self {
            Self::Page(track, effect, _)
            | Self::Visual(track, effect, _)
            | Self::SidechainInput(track, effect, _) => (*track, *effect),
        }
    }
}

pub struct CompressorPanel {
    pub page: CompressorPage,
    pub visual: CompressorVisual,
    pub sidechain_input: ExternalInputId,
    pub meter: DynamicsMeter,
    history: [f32; HISTORY_WINDOWS],
    next: usize,
    count: usize,
    last_meter_at: Option<Instant>,
}
impl Default for CompressorPanel {
    fn default() -> Self {
        Self {
            page: Default::default(),
            visual: Default::default(),
            sidechain_input: ExternalInputId(0),
            meter: Default::default(),
            history: [0.0; HISTORY_WINDOWS],
            next: 0,
            count: 0,
            last_meter_at: None,
        }
    }
}
impl CompressorPanel {
    pub fn update(&mut self, message: CompressorMsg) {
        match message {
            CompressorMsg::Page(_, _, page) => self.page = page,
            CompressorMsg::Visual(_, _, visual) => self.visual = visual,
            CompressorMsg::SidechainInput(_, _, input) => self.sidechain_input = input,
        }
    }
    pub fn accept(&mut self, meter: DynamicsMeter, now: Instant) {
        if meter
            .current_reduction_db
            .iter()
            .chain(&meter.peak_reduction_db)
            .chain(&meter.detector_peak)
            .any(|value| !value.is_finite() || *value < 0.0)
        {
            return;
        }
        self.meter = meter;
        self.history[self.next] = meter.peak_reduction_db[0].max(meter.peak_reduction_db[1]);
        self.next = (self.next + 1) % HISTORY_WINDOWS;
        self.count = (self.count + 1).min(HISTORY_WINDOWS);
        self.last_meter_at = Some(now);
    }
    pub fn history(&self) -> impl Iterator<Item = f32> + '_ {
        (0..self.count).map(|index| {
            self.history[(self.next + HISTORY_WINDOWS - self.count + index) % HISTORY_WINDOWS]
        })
    }
    pub fn reset_metering(&mut self) {
        self.meter = Default::default();
        self.history.fill(0.0);
        self.next = 0;
        self.count = 0;
        self.last_meter_at = None;
    }
    pub fn reset_stale(&mut self, now: Instant) {
        if self
            .last_meter_at
            .is_some_and(|last| now.saturating_duration_since(last) >= Duration::from_millis(250))
        {
            self.reset_metering();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_keeps_actual_window_peaks_in_order_with_fixed_capacity() {
        let now = Instant::now();
        let mut panel = CompressorPanel::default();
        for index in 0..HISTORY_WINDOWS + 7 {
            panel.accept(
                DynamicsMeter {
                    current_reduction_db: [0.25, 0.5],
                    peak_reduction_db: [index as f32, 0.5],
                    detector_peak: [0.2, 0.1],
                },
                now,
            );
        }
        assert_eq!(
            panel.history().collect::<Vec<_>>(),
            (7..HISTORY_WINDOWS + 7)
                .map(|index| index as f32)
                .collect::<Vec<_>>()
        );
        assert_eq!(panel.meter.current_reduction_db, [0.25, 0.5]);
        assert_eq!(panel.history.len(), HISTORY_WINDOWS);
    }
    #[test]
    fn view_choices_survive_stale_meter_resets_and_selection_resubscription() {
        let (track, effect) = (TrackId::new(), EffectId::new());
        let now = Instant::now();
        let mut panel = CompressorPanel::default();
        panel.update(CompressorMsg::Page(
            track,
            effect,
            CompressorPage::Sidechain,
        ));
        panel.update(CompressorMsg::Visual(
            track,
            effect,
            CompressorVisual::History,
        ));
        panel.update(CompressorMsg::SidechainInput(
            track,
            effect,
            ExternalInputId(3),
        ));
        panel.accept(
            DynamicsMeter {
                current_reduction_db: [2.0, 3.0],
                peak_reduction_db: [4.0, 5.0],
                detector_peak: [0.5, 0.25],
            },
            now,
        );
        panel.reset_stale(now + Duration::from_millis(249));
        assert_eq!(panel.history().count(), 1);
        panel.reset_stale(now + Duration::from_millis(250));
        assert_eq!(panel.meter, DynamicsMeter::default());
        assert_eq!(panel.history().count(), 0);
        assert_eq!(panel.page, CompressorPage::Sidechain);
        assert_eq!(panel.visual, CompressorVisual::History);
        assert_eq!(panel.sidechain_input, ExternalInputId(3));
        panel.reset_metering();
        assert_eq!(panel.page, CompressorPage::Sidechain);
    }
    #[test]
    fn invalid_measurements_cannot_poison_the_display() {
        let mut panel = CompressorPanel::default();
        panel.accept(
            DynamicsMeter {
                current_reduction_db: [f32::NAN, 0.0],
                ..Default::default()
            },
            Instant::now(),
        );
        panel.accept(
            DynamicsMeter {
                peak_reduction_db: [-1.0, 0.0],
                ..Default::default()
            },
            Instant::now(),
        );
        assert_eq!(panel.history().count(), 0);
        assert_eq!(panel.meter, DynamicsMeter::default());
    }
}
