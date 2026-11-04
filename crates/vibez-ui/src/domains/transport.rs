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
use crate::state::{PlaybackPhase, TransportState};

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
    /// The engine stopped, whether asked to or at the song end.
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
    /// Perform holds one project BPM until its Section transport stops.
    pub perform_tempo_locked: bool,
    /// Perform owns the running playback source, so Stop must not seek the
    /// independent Arrange cursor back to zero.
    pub perform_playback_active: bool,
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
    TempoRejected,
}

impl TransportState {
    /// What the screen shows, so it never runs ahead of the engine.
    pub fn is_playing(&self) -> bool {
        matches!(
            self.playback,
            PlaybackPhase::Playing | PlaybackPhase::Stopping
        )
    }

    /// Playing, or asked to play: what a toggle acts on before the engine
    /// confirms.
    pub fn will_play(&self) -> bool {
        matches!(
            self.playback,
            PlaybackPhase::Starting | PlaybackPhase::Playing
        )
    }

    /// The engine reported its transport. A request it has not reached yet,
    /// such as a Stop sent right after Play, stays pending.
    pub fn follow_engine(&mut self, playing: bool) {
        self.playback = match (self.playback, playing) {
            (PlaybackPhase::Starting, false) => PlaybackPhase::Starting,
            (PlaybackPhase::Stopping, true) => PlaybackPhase::Stopping,
            (_, true) => PlaybackPhase::Playing,
            (_, false) => PlaybackPhase::Stopped,
        };
    }

    pub fn samples_per_beat(&self) -> f64 {
        vibez_core::time::TempoMap::new(self.bpm, self.sample_rate).samples_per_beat()
    }

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
                if self.playback != PlaybackPhase::Playing {
                    self.playback = PlaybackPhase::Starting;
                }
                engine.send(EngineCommand::Play);
                TransportAction::None
            }
            TransportMsg::Stop => {
                let already_stopped = !self.will_play();
                if self.playback != PlaybackPhase::Stopped {
                    self.playback = PlaybackPhase::Stopping;
                }
                engine.send(EngineCommand::Stop);
                if !ctx.perform_playback_active {
                    if already_stopped {
                        self.play_start_samples = 0;
                    }
                    self.return_to_play_start(engine);
                }
                TransportAction::None
            }
            TransportMsg::TogglePlayback => {
                let next = if self.will_play() {
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
                    self.place_play_start(sample_pos, engine);
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
                self.place_play_start(sample_pos, engine);
                TransportAction::ClearTimeSelection
            }
            TransportMsg::BpmChanged(text) => {
                self.bpm_text = text;
                TransportAction::None
            }
            TransportMsg::BpmSubmit => match self.bpm_text.parse::<f64>() {
                Ok(_) if ctx.perform_tempo_locked => {
                    self.bpm_text = format!("{:.0}", self.bpm);
                    TransportAction::TempoRejected
                }
                Ok(bpm) => self.set_bpm(bpm, engine),
                Err(_) => {
                    self.bpm_text = format!("{:.0}", self.bpm);
                    TransportAction::None
                }
            },
            TransportMsg::NudgeBpm(delta) => {
                if ctx.perform_tempo_locked {
                    self.bpm_text = format!("{:.0}", self.bpm);
                    return TransportAction::TempoRejected;
                }
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
                let stopped_on_its_own = self.will_play();
                self.follow_engine(false);
                if stopped_on_its_own && !ctx.perform_playback_active {
                    self.return_to_play_start(engine);
                }
                TransportAction::None
            }
        }
    }

    fn place_play_start(&mut self, samples: u64, engine: &mut impl EngineHandle) {
        self.play_start_samples = samples;
        self.return_to_play_start(engine);
    }

    fn return_to_play_start(&mut self, engine: &mut impl EngineHandle) {
        self.position_samples = self.play_start_samples;
        engine.send(EngineCommand::Seek(self.play_start_samples));
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
    fn play_sends_play() {
        let mut t = transport();
        let mut engine = RecordingEngine::default();
        t.update(TransportMsg::Play, &mut engine, TransportCtx::default());
        assert!(matches!(engine.0[0], EngineCommand::Play));
    }

    #[test]
    fn stop_before_any_click_returns_to_the_song_start() {
        let mut t = transport();
        t.playback = PlaybackPhase::Playing;
        t.position_samples = 12345;
        let mut engine = RecordingEngine::default();
        t.update(TransportMsg::Stop, &mut engine, TransportCtx::default());
        assert!(!t.will_play());
        assert_eq!(t.position_samples, 0);
        assert!(matches!(engine.0[1], EngineCommand::Seek(0)));
    }

    /// Clicks `beat`, plays, and lets the playhead run on past it.
    fn playing_from(beat: f64) -> (TransportState, RecordingEngine) {
        let mut t = transport();
        let mut engine = RecordingEngine::default();
        t.update(
            TransportMsg::SeekToBeat(beat),
            &mut engine,
            TransportCtx::default(),
        );
        t.update(TransportMsg::Play, &mut engine, TransportCtx::default());
        t.follow_engine(true);
        let ran_on = t.position_samples + 100_000;
        t.update(
            TransportMsg::EnginePosition(ran_on),
            &mut engine,
            TransportCtx::default(),
        );
        engine.0.clear();
        (t, engine)
    }

    #[test]
    fn stop_returns_to_where_play_started() {
        let (mut t, mut engine) = playing_from(128.0);
        let start = t.beats_to_samples_at(128.0);

        t.update(TransportMsg::Stop, &mut engine, TransportCtx::default());

        assert_eq!(t.position_samples, start);
        assert!(matches!(
            engine.0.as_slice(),
            [EngineCommand::Stop, EngineCommand::Seek(s)] if *s == start
        ));
    }

    #[test]
    fn a_click_while_playing_moves_where_stop_returns() {
        let (mut t, mut engine) = playing_from(128.0);
        t.update(
            TransportMsg::SeekToBeat(64.0),
            &mut engine,
            TransportCtx::default(),
        );
        t.update(
            TransportMsg::EnginePosition(t.position_samples + 100_000),
            &mut engine,
            TransportCtx::default(),
        );

        t.update(TransportMsg::Stop, &mut engine, TransportCtx::default());

        assert_eq!(t.position_samples, t.beats_to_samples_at(64.0));
    }

    #[test]
    fn stopping_on_its_own_returns_to_where_play_started() {
        let (mut t, mut engine) = playing_from(128.0);
        let start = t.beats_to_samples_at(128.0);

        t.update(
            TransportMsg::EngineStopped,
            &mut engine,
            TransportCtx::default(),
        );

        assert!(!t.is_playing());
        assert_eq!(t.position_samples, start);
        assert!(matches!(engine.0.as_slice(), [EngineCommand::Seek(s)] if *s == start));
    }

    #[test]
    fn the_confirmation_of_a_requested_stop_does_not_seek_again() {
        let (mut t, mut engine) = playing_from(128.0);
        t.update(TransportMsg::Stop, &mut engine, TransportCtx::default());
        engine.0.clear();

        t.update(
            TransportMsg::EngineStopped,
            &mut engine,
            TransportCtx::default(),
        );

        assert!(!t.is_playing());
        assert!(engine.0.is_empty());
    }

    #[test]
    fn perform_stopping_on_its_own_keeps_the_arrange_cursor() {
        let (mut t, mut engine) = playing_from(128.0);
        let cursor = t.position_samples;

        t.update(
            TransportMsg::EngineStopped,
            &mut engine,
            TransportCtx {
                perform_playback_active: true,
                ..TransportCtx::default()
            },
        );

        assert!(!t.is_playing());
        assert_eq!(t.position_samples, cursor);
        assert!(engine.0.is_empty());
    }

    #[test]
    fn a_second_stop_goes_to_the_song_start_and_play_starts_there() {
        let (mut t, mut engine) = playing_from(128.0);
        t.update(TransportMsg::Stop, &mut engine, TransportCtx::default());
        t.follow_engine(false);
        assert_eq!(t.position_samples, t.beats_to_samples_at(128.0));

        engine.0.clear();
        t.update(TransportMsg::Stop, &mut engine, TransportCtx::default());
        assert_eq!(t.position_samples, 0);
        assert!(matches!(
            engine.0.as_slice(),
            [EngineCommand::Stop, EngineCommand::Seek(0)]
        ));

        t.update(TransportMsg::Play, &mut engine, TransportCtx::default());
        t.follow_engine(true);
        t.update(
            TransportMsg::EnginePosition(50_000),
            &mut engine,
            TransportCtx::default(),
        );
        t.update(TransportMsg::Stop, &mut engine, TransportCtx::default());
        assert_eq!(t.position_samples, 0);
    }

    #[test]
    fn continue_plays_from_where_playback_stopped_and_stop_still_returns() {
        let (mut t, mut engine) = playing_from(128.0);
        let stopped_at = t.position_samples;
        t.update(TransportMsg::Stop, &mut engine, TransportCtx::default());
        t.follow_engine(false);
        engine.0.clear();

        t.update(
            TransportMsg::ContinuePlayback,
            &mut engine,
            TransportCtx::default(),
        );
        assert_eq!(t.position_samples, stopped_at);
        assert!(matches!(
            engine.0.as_slice(),
            [EngineCommand::Seek(s), EngineCommand::Play] if *s == stopped_at
        ));

        t.follow_engine(true);
        t.update(
            TransportMsg::EnginePosition(stopped_at + 1_000),
            &mut engine,
            TransportCtx::default(),
        );
        t.update(TransportMsg::Stop, &mut engine, TransportCtx::default());
        assert_eq!(t.position_samples, t.beats_to_samples_at(128.0));
    }

    #[test]
    fn continue_after_a_click_plays_from_the_click() {
        let (mut t, mut engine) = playing_from(128.0);
        t.update(TransportMsg::Stop, &mut engine, TransportCtx::default());
        t.follow_engine(false);
        t.update(
            TransportMsg::SeekToBeat(64.0),
            &mut engine,
            TransportCtx::default(),
        );

        t.update(
            TransportMsg::ContinuePlayback,
            &mut engine,
            TransportCtx::default(),
        );

        assert_eq!(t.position_samples, t.beats_to_samples_at(64.0));
    }

    #[test]
    fn continue_after_stopping_on_its_own_plays_from_where_it_stopped() {
        let (mut t, mut engine) = playing_from(128.0);
        let stopped_at = t.position_samples;
        t.update(
            TransportMsg::EngineStopped,
            &mut engine,
            TransportCtx::default(),
        );

        t.update(
            TransportMsg::ContinuePlayback,
            &mut engine,
            TransportCtx::default(),
        );

        assert_eq!(t.position_samples, stopped_at);
    }

    #[test]
    fn continue_while_playing_stops_like_space() {
        let (mut t, mut engine) = playing_from(128.0);

        t.update(
            TransportMsg::ContinuePlayback,
            &mut engine,
            TransportCtx::default(),
        );

        assert!(!t.will_play());
        assert_eq!(t.position_samples, t.beats_to_samples_at(128.0));
    }

    #[test]
    fn perform_stop_preserves_the_arrange_cursor_and_does_not_seek() {
        let mut t = transport();
        t.playback = PlaybackPhase::Playing;
        t.position_samples = 12_345;
        let mut engine = RecordingEngine::default();
        t.update(
            TransportMsg::Stop,
            &mut engine,
            TransportCtx {
                perform_playback_active: true,
                ..TransportCtx::default()
            },
        );

        assert!(!t.will_play());
        assert_eq!(t.position_samples, 12_345);
        assert!(matches!(engine.0.as_slice(), [EngineCommand::Stop]));
    }

    #[test]
    fn play_shows_playing_only_once_the_engine_starts() {
        let mut t = transport();
        let mut engine = RecordingEngine::default();
        t.update(TransportMsg::Play, &mut engine, TransportCtx::default());
        assert!(!t.is_playing());
        assert!(t.will_play());

        t.follow_engine(true);
        assert!(t.is_playing());
        assert!(t.will_play());
    }

    #[test]
    fn stop_shows_stopped_only_once_the_engine_stops() {
        let mut t = transport();
        t.playback = PlaybackPhase::Playing;
        let mut engine = RecordingEngine::default();
        t.update(TransportMsg::Stop, &mut engine, TransportCtx::default());
        assert!(t.is_playing());
        assert!(!t.will_play());

        t.follow_engine(false);
        assert!(!t.is_playing());
    }

    #[test]
    fn toggling_before_the_engine_answers_follows_the_request() {
        let mut t = transport();
        let mut engine = RecordingEngine::default();
        t.update(
            TransportMsg::TogglePlayback,
            &mut engine,
            TransportCtx::default(),
        );
        t.update(
            TransportMsg::TogglePlayback,
            &mut engine,
            TransportCtx::default(),
        );
        assert!(matches!(
            engine.0.as_slice(),
            [
                EngineCommand::Play,
                EngineCommand::Stop,
                EngineCommand::Seek(0)
            ]
        ));

        t.follow_engine(true);
        assert!(t.is_playing());
        assert!(
            !t.will_play(),
            "the Stop sent after Play is still on its way"
        );
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
    fn perform_playback_rejects_bpm_submit() {
        let mut t = transport();
        t.bpm_text = "140".into();
        let mut engine = RecordingEngine::default();

        let action = t.update(
            TransportMsg::BpmSubmit,
            &mut engine,
            TransportCtx {
                perform_tempo_locked: true,
                ..TransportCtx::default()
            },
        );

        assert_eq!(t.bpm, 120.0);
        assert_eq!(t.bpm_text, "120");
        assert_eq!(action, TransportAction::TempoRejected);
        assert!(engine.0.is_empty());
    }

    #[test]
    fn seek_clamps_and_requests_selection_clear() {
        let mut t = transport();
        let mut engine = RecordingEngine::default();
        let ctx = TransportCtx {
            total_duration_samples: 1000,
            time_selection: None,
            ..TransportCtx::default()
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
            ..TransportCtx::default()
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
