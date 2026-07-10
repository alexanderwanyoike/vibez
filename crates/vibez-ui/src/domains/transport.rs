//! Transport domain: playback, tempo, and the arrangement loop.
//!
//! The reference implementation of the domain pattern. `update`
//! receives only the transport slice, an engine handle, and a small
//! read-only context of facts from other domains; anything it cannot
//! do itself is returned as a [`TransportAction`] for app.rs to
//! route. That boundary is what isolates bugs and makes this file
//! unit-testable without iced.

use vibez_engine::commands::EngineCommand;

use super::EngineHandle;
use crate::state::TransportState;

/// Messages the transport domain handles.
#[derive(Debug, Clone)]
pub enum TransportMsg {
    Play,
    Stop,
    TogglePlayback,
    /// Seek to a normalized [0, 1] position on the timeline.
    Seek(f64),
    /// Seek to an absolute musical position on the arrangement ruler.
    SeekToBeat(f64),
    BpmChanged(String),
    BpmSubmit,
    /// Increment/decrement project BPM by a whole beat-per-minute.
    NudgeBpm(f64),
    ToggleArrangementLoop,
    SetArrangementLoopRegion {
        start_beats: f64,
        end_beats: f64,
    },
    /// Engine events routed to this domain.
    EnginePosition(u64),
    EngineStopped,
}

/// Read-only facts from other domains that transport decisions need.
/// Computed by the router; keeps this module free of arrangement
/// internals.
#[derive(Debug, Clone, Copy, Default)]
pub struct TransportCtx {
    /// Total arrangement length in samples (for Seek normalization).
    pub total_duration_samples: u64,
    /// Active time selection in beats, if any (loop-enable copies it).
    pub time_selection: Option<(f64, f64)>,
}

/// Cross-domain effects the transport cannot perform itself. The
/// router (app.rs) translates these into calls on other domains.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransportAction {
    None,
    /// A seek happened; the arrangement should clear its time selection.
    ClearTimeSelection,
    /// The project tempo changed; warped clips must follow.
    TempoChanged {
        old_bpm: f64,
        new_bpm: f64,
    },
}

impl TransportState {
    /// Beat position -> absolute sample position at the current tempo.
    pub fn beats_to_samples_at(&self, beats: f64) -> u64 {
        if self.bpm > 0.0 {
            (beats * self.sample_rate as f64 * 60.0 / self.bpm) as u64
        } else {
            0
        }
    }

    pub fn update(
        &mut self,
        msg: TransportMsg,
        engine: &mut impl EngineHandle,
        ctx: TransportCtx,
    ) -> TransportAction {
        match msg {
            TransportMsg::Play => {
                self.playing = true;
                engine.send(EngineCommand::Play);
                TransportAction::None
            }
            TransportMsg::Stop => {
                self.playing = false;
                self.position_samples = 0;
                engine.send(EngineCommand::Stop);
                engine.send(EngineCommand::Seek(0));
                TransportAction::None
            }
            TransportMsg::TogglePlayback => {
                let next = if self.playing {
                    TransportMsg::Stop
                } else {
                    TransportMsg::Play
                };
                self.update(next, engine, ctx)
            }
            TransportMsg::Seek(normalized) => {
                if ctx.total_duration_samples > 0 {
                    let sample_pos =
                        (normalized.clamp(0.0, 1.0) * ctx.total_duration_samples as f64) as u64;
                    self.position_samples = sample_pos;
                    engine.send(EngineCommand::Seek(sample_pos));
                }
                TransportAction::ClearTimeSelection
            }
            TransportMsg::SeekToBeat(beat) => {
                let samples_per_beat = if self.bpm > 0.0 {
                    self.sample_rate as f64 * 60.0 / self.bpm
                } else {
                    0.0
                };
                let sample_pos = (beat.max(0.0) * samples_per_beat).round() as u64;
                self.position_samples = sample_pos;
                engine.send(EngineCommand::Seek(sample_pos));
                TransportAction::ClearTimeSelection
            }
            TransportMsg::BpmChanged(text) => {
                self.bpm_text = text;
                TransportAction::None
            }
            TransportMsg::BpmSubmit => match self.bpm_text.parse::<f64>() {
                Ok(bpm) => self.set_bpm(bpm, engine),
                Err(_) => {
                    self.bpm_text = format!("{:.0}", self.bpm);
                    TransportAction::None
                }
            },
            TransportMsg::NudgeBpm(delta) => {
                let bpm = self.bpm + delta;
                self.set_bpm(bpm, engine)
            }
            TransportMsg::ToggleArrangementLoop => {
                self.loop_enabled = !self.loop_enabled;
                engine.send(EngineCommand::SetArrangementLoop(self.loop_enabled));
                if self.loop_enabled {
                    // Enabling the loop adopts an active time selection.
                    if let Some((start, end)) = ctx.time_selection {
                        if end > start {
                            self.loop_start_beats = start;
                            self.loop_end_beats = end;
                        }
                    }
                    self.send_loop_region(engine);
                }
                TransportAction::None
            }
            TransportMsg::SetArrangementLoopRegion {
                start_beats,
                end_beats,
            } => {
                self.loop_start_beats = start_beats;
                self.loop_end_beats = end_beats;
                self.send_loop_region(engine);
                TransportAction::None
            }
            TransportMsg::EnginePosition(pos) => {
                self.position_samples = pos;
                TransportAction::None
            }
            TransportMsg::EngineStopped => {
                self.playing = false;
                TransportAction::None
            }
        }
    }

    /// Apply a new tempo: clamp, notify the engine, remap the loop
    /// region, and report the change so warped clips can follow.
    fn set_bpm(&mut self, bpm: f64, engine: &mut impl EngineHandle) -> TransportAction {
        let bpm = bpm.clamp(20.0, 999.0);
        let old_bpm = self.bpm;
        self.bpm = bpm;
        self.bpm_text = format!("{bpm:.0}");
        engine.send(EngineCommand::SetBpm(bpm));
        if self.loop_enabled {
            self.send_loop_region(engine);
        }
        if (bpm - old_bpm).abs() > f64::EPSILON {
            TransportAction::TempoChanged {
                old_bpm,
                new_bpm: bpm,
            }
        } else {
            TransportAction::None
        }
    }

    fn send_loop_region(&self, engine: &mut impl EngineHandle) {
        let start = self.beats_to_samples_at(self.loop_start_beats);
        let end = self.beats_to_samples_at(self.loop_end_beats);
        engine.send(EngineCommand::SetArrangementLoopRegion { start, end });
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::RecordingEngine;
    use super::*;

    fn transport() -> TransportState {
        TransportState::default()
    }

    #[test]
    fn play_sends_play_and_sets_state() {
        let mut t = transport();
        let mut engine = RecordingEngine::default();
        t.update(TransportMsg::Play, &mut engine, TransportCtx::default());
        assert!(t.playing);
        assert!(matches!(engine.0[0], EngineCommand::Play));
    }

    #[test]
    fn stop_resets_position_and_seeks_zero() {
        let mut t = transport();
        t.playing = true;
        t.position_samples = 12345;
        let mut engine = RecordingEngine::default();
        t.update(TransportMsg::Stop, &mut engine, TransportCtx::default());
        assert!(!t.playing);
        assert_eq!(t.position_samples, 0);
        assert!(matches!(engine.0[1], EngineCommand::Seek(0)));
    }

    #[test]
    fn bpm_submit_parses_clamps_and_reports_change() {
        let mut t = transport();
        t.bpm_text = "1500".to_string();
        let mut engine = RecordingEngine::default();
        let action = t.update(
            TransportMsg::BpmSubmit,
            &mut engine,
            TransportCtx::default(),
        );
        assert_eq!(t.bpm, 999.0);
        assert_eq!(
            action,
            TransportAction::TempoChanged {
                old_bpm: 120.0,
                new_bpm: 999.0
            }
        );
        assert!(matches!(engine.0[0], EngineCommand::SetBpm(b) if (b - 999.0).abs() < 1e-9));
    }

    #[test]
    fn garbage_bpm_text_restores_current_tempo() {
        let mut t = transport();
        t.bpm_text = "not a number".to_string();
        let mut engine = RecordingEngine::default();
        let action = t.update(
            TransportMsg::BpmSubmit,
            &mut engine,
            TransportCtx::default(),
        );
        assert_eq!(action, TransportAction::None);
        assert_eq!(t.bpm_text, "120");
        assert!(engine.0.is_empty());
    }

    #[test]
    fn seek_clamps_and_requests_selection_clear() {
        let mut t = transport();
        let mut engine = RecordingEngine::default();
        let ctx = TransportCtx {
            total_duration_samples: 1000,
            time_selection: None,
        };
        let action = t.update(TransportMsg::Seek(2.0), &mut engine, ctx);
        assert_eq!(t.position_samples, 1000);
        assert_eq!(action, TransportAction::ClearTimeSelection);
    }

    #[test]
    fn beat_seek_uses_absolute_ruler_position() {
        let mut t = transport();
        let mut engine = RecordingEngine::default();

        let action = t.update(
            TransportMsg::SeekToBeat(3.5),
            &mut engine,
            TransportCtx::default(),
        );

        assert_eq!(t.position_samples, 77_175);
        assert_eq!(action, TransportAction::ClearTimeSelection);
        assert!(matches!(engine.0[0], EngineCommand::Seek(77_175)));
    }

    #[test]
    fn enabling_loop_adopts_time_selection() {
        let mut t = transport();
        let mut engine = RecordingEngine::default();
        let ctx = TransportCtx {
            total_duration_samples: 0,
            time_selection: Some((4.0, 8.0)),
        };
        t.update(TransportMsg::ToggleArrangementLoop, &mut engine, ctx);
        assert!(t.loop_enabled);
        assert_eq!((t.loop_start_beats, t.loop_end_beats), (4.0, 8.0));
        assert!(matches!(
            engine.0[1],
            EngineCommand::SetArrangementLoopRegion { .. }
        ));
    }

    #[test]
    fn nudge_from_spinner_reports_tempo_change() {
        let mut t = transport();
        let mut engine = RecordingEngine::default();
        let action = t.update(
            TransportMsg::NudgeBpm(1.0),
            &mut engine,
            TransportCtx::default(),
        );
        assert_eq!(
            action,
            TransportAction::TempoChanged {
                old_bpm: 120.0,
                new_bpm: 121.0
            }
        );
    }
}
