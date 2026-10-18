//! What a native Gate card shows besides its parameters: whether Advanced
//! is open, which Advanced group a narrow card shows, and the levels and
//! gate decisions the engine measured.

use std::time::{Duration, Instant};

use vibez_core::effect::{DynamicsMeter, GatePhase};
use vibez_core::id::EffectId;
use vibez_engine::metering::MeterValues;

use super::device_levels::ReadingHistory;

/// How far back the history reaches.
pub const HISTORY: Duration = Duration::from_secs(4);

pub fn is_native_gate(effect: &crate::state::UiEffect) -> bool {
    effect.effect_type == vibez_core::effect::EffectType::Gate
        && effect.plugin_ref.is_none()
        && effect.plugin_name.is_none()
}

/// Advanced's two groups when there is room for only one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AdvancedPage {
    #[default]
    Response,
    Detector,
}

#[derive(Debug, Clone)]
pub enum GateMsg {
    ToggleAdvanced(EffectId),
    ShowAdvancedPage(EffectId, AdvancedPage),
}

impl GateMsg {
    pub fn effect_id(&self) -> EffectId {
        match self {
            Self::ToggleAdvanced(id) | Self::ShowAdvancedPage(id, _) => *id,
        }
    }
}

/// What the gate heard and did since the reading before.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GateReading {
    /// The loudest the detector got.
    pub level_db: f32,
    /// It opened or held at some point, however briefly.
    pub open: bool,
    /// Its deepest attenuation, in positive dB.
    pub reduction_db: f32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GatePanelState {
    pub advanced: bool,
    pub advanced_page: AdvancedPage,
    pub input: MeterValues,
    pub output: MeterValues,
    pub phase: GatePhase,
    pub history: ReadingHistory<GateReading>,
    pub last_meter_at: Option<Instant>,
}

impl GatePanelState {
    pub fn update(&mut self, message: GateMsg) {
        match message {
            GateMsg::ToggleAdvanced(_) => self.advanced = !self.advanced,
            GateMsg::ShowAdvancedPage(_, page) => self.advanced_page = page,
        }
    }

    pub fn accept_dynamics(&mut self, meter: DynamicsMeter, now: Instant) {
        let Some(gate) = meter.gate else {
            return;
        };
        self.phase = gate.phase;
        self.last_meter_at = Some(now);
        let opened = GatePhase::Open as u8 | GatePhase::Hold as u8;
        self.history.push(
            now,
            GateReading {
                level_db: gate.peak_envelope_db,
                open: gate.phases_seen & opened != 0,
                reduction_db: meter.peak_reduction_db[0]
                    .max(meter.peak_reduction_db[1])
                    .max(0.0),
            },
            HISTORY,
        );
    }
}

impl super::device_levels::MeteredPanel for GatePanelState {
    fn accept_levels(&mut self, input: MeterValues, output: MeterValues, now: Instant) {
        self.input = input;
        self.output = output;
        self.last_meter_at = Some(now);
    }

    fn reset_stale(&mut self, now: Instant) {
        if self
            .last_meter_at
            .is_some_and(|last| now.saturating_duration_since(last) >= Duration::from_millis(250))
        {
            self.clear_readings();
        }
    }

    fn clear_readings(&mut self) {
        self.input = MeterValues::SILENT;
        self.output = MeterValues::SILENT;
        self.phase = GatePhase::Closed;
        self.history.clear();
        self.last_meter_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::device_levels::MeteredPanel;
    use vibez_core::effect::GateMeter;

    fn meter(phase: GatePhase, phases_seen: u8, envelope_db: f32) -> DynamicsMeter {
        DynamicsMeter {
            peak_reduction_db: [12.0, 30.0],
            gate: Some(GateMeter {
                phase,
                peak_envelope_db: envelope_db,
                phases_seen,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn a_brief_opening_between_readings_still_shows_as_open() {
        let now = Instant::now();
        let mut panel = GatePanelState::default();
        let seen = GatePhase::Open as u8 | GatePhase::Closed as u8;
        panel.accept_dynamics(meter(GatePhase::Closed, seen, -20.0), now);
        assert_eq!(panel.phase, GatePhase::Closed);
        let (_, reading) = panel.history.iter().next().unwrap();
        assert_eq!(
            *reading,
            GateReading {
                level_db: -20.0,
                open: true,
                reduction_db: 30.0,
            }
        );
        panel.accept_dynamics(
            meter(GatePhase::Closed, GatePhase::Closed as u8, -60.0),
            now,
        );
        assert!(!panel.history.iter().last().unwrap().1.open);
    }

    #[test]
    fn readings_without_a_gate_meter_are_ignored() {
        let mut panel = GatePanelState::default();
        panel.accept_dynamics(DynamicsMeter::default(), Instant::now());
        assert!(panel.history.is_empty() && panel.last_meter_at.is_none());
    }

    #[test]
    fn stale_readings_clear_and_panel_choices_stay() {
        let now = Instant::now();
        let id = EffectId::new();
        let mut panel = GatePanelState::default();
        panel.update(GateMsg::ToggleAdvanced(id));
        panel.update(GateMsg::ShowAdvancedPage(id, AdvancedPage::Detector));
        panel.accept_dynamics(meter(GatePhase::Open, GatePhase::Open as u8, -10.0), now);
        panel.reset_stale(now + Duration::from_millis(249));
        assert_eq!(panel.phase, GatePhase::Open);
        panel.reset_stale(now + Duration::from_millis(250));
        assert_eq!(panel.phase, GatePhase::Closed);
        assert!(panel.history.is_empty());
        assert!(panel.advanced && panel.advanced_page == AdvancedPage::Detector);
    }
}
