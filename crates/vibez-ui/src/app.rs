use std::path::PathBuf;
use std::sync::Arc;

use iced::widget::{
    button, canvas, center, column, container, horizontal_space, row, scrollable, text, text_input,
};
use iced::{Color, Element, Length, Subscription, Task, Theme};

use rtrb::{Consumer, Producer};
use vibez_audio_io::audio_stream::AudioOutputStream;
use vibez_audio_io::file_io;
use vibez_core::constants::UI_TICK_MS;
use vibez_core::effect::EffectType;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::{InstrumentKind, MidiNote, TrackKind};
use vibez_engine::commands::EngineCommand;
use vibez_engine::engine::AudioEngine;
use vibez_engine::events::EngineEvent;

use crate::icons;
use crate::message::Message;
use crate::state::{
    AppState, ArrangementSelection, DetailPanelTab, UiClip, UiEffect, UiNoteClip, UiTrack,
    Workspace,
};
use crate::theme as th;
use crate::widgets::audio_clip_detail::AudioClipDetailWidget;
use crate::widgets::effect_slot::view_effect_slot;
use crate::widgets::mixer_strip::view_mixer_strip;
use crate::widgets::piano_roll::PianoRollWidget;
use crate::widgets::timeline::{RulerWidget, TrackClipCanvas};
use crate::widgets::track_header::view_track_header;
use crate::widgets::vu_meter::VuMeterWidget;

struct App {
    state: AppState,
    cmd_tx: Option<Producer<EngineCommand>>,
    event_rx: Option<Consumer<EngineEvent>>,
    _stream: Option<AudioOutputStream>,
}

pub fn run() -> iced::Result {
    iced::application("vibez", App::update, App::view)
        .theme(App::theme)
        .subscription(App::subscription)
        .window_size((1400.0, 900.0))
        .font(icons::ICON_FONT_BYTES)
        .run_with(App::new)
}

impl App {
    fn new() -> (Self, Task<Message>) {
        let (engine, cmd_tx, event_rx) = AudioEngine::new();

        let (stream, sample_rate) = match AudioOutputStream::open(engine) {
            Ok(s) => {
                let sr = s.sample_rate();
                if let Err(e) = s.play() {
                    eprintln!("vibez: failed to start audio stream: {e}");
                }
                (Some(s), sr)
            }
            Err(e) => {
                eprintln!("vibez: failed to open audio stream: {e}");
                (None, 44_100)
            }
        };

        let state = AppState {
            sample_rate,
            ..Default::default()
        };

        let mut app = Self {
            state,
            cmd_tx: Some(cmd_tx),
            event_rx: Some(event_rx),
            _stream: stream,
        };

        // Inform the engine of the actual sample rate
        app.send_command(EngineCommand::SetBpm(app.state.bpm));

        (app, Task::none())
    }

    fn send_command(&mut self, cmd: EngineCommand) {
        if let Some(ref mut tx) = self.cmd_tx {
            let _ = tx.push(cmd);
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Play => {
                self.state.playing = true;
                self.send_command(EngineCommand::Play);
            }
            Message::Stop => {
                self.state.playing = false;
                self.state.position_samples = 0;
                self.send_command(EngineCommand::Stop);
                self.send_command(EngineCommand::Seek(0));
            }
            Message::TogglePlayback => {
                if self.state.playing {
                    return self.update(Message::Stop);
                } else {
                    return self.update(Message::Play);
                }
            }
            Message::Seek(normalized) => {
                let total = self.state.total_duration_samples();
                if total > 0 {
                    let sample_pos = (normalized * total as f64) as u64;
                    self.state.position_samples = sample_pos;
                    self.send_command(EngineCommand::Seek(sample_pos));
                }
            }
            Message::BpmChanged(val) => {
                self.state.bpm_text = val;
            }
            Message::BpmSubmit => {
                if let Ok(bpm) = self.state.bpm_text.parse::<f64>() {
                    let bpm = bpm.clamp(20.0, 999.0);
                    self.state.bpm = bpm;
                    self.state.bpm_text = format!("{bpm:.0}");
                    self.send_command(EngineCommand::SetBpm(bpm));
                } else {
                    let bpm = self.state.bpm;
                    self.state.bpm_text = format!("{bpm:.0}");
                }
            }

            // -- Workspace --
            Message::SwitchWorkspace(ws) => {
                self.state.workspace = ws;
            }

            Message::SwitchDetailTab(tab) => {
                self.state.detail_panel_tab = tab;
            }

            // -- Zoom / scroll --
            Message::ZoomIn => {
                self.state.zoom_level = (self.state.zoom_level * 1.25).min(16.0);
            }
            Message::ZoomOut => {
                self.state.zoom_level = (self.state.zoom_level / 1.25).max(0.25);
            }
            Message::SetZoom(level) => {
                self.state.zoom_level = level.clamp(0.25, 16.0);
            }
            Message::ScrollArrangement(delta) => {
                let total = self.state.total_beats();
                self.state.scroll_offset_beats =
                    (self.state.scroll_offset_beats + delta).clamp(0.0, total);
            }

            // -- Snap grid --
            Message::SetSnapGrid(grid) => {
                self.state.snap_grid = grid;
            }

            // -- Engine events --
            Message::Tick => {
                self.poll_engine_events();
            }
            Message::EnginePosition(pos) => {
                self.state.position_samples = pos;
            }
            Message::EngineMetering { peak_l, peak_r } => {
                self.state.peak_l = peak_l;
                self.state.peak_r = peak_r;
            }
            Message::EngineStopped => {
                self.state.playing = false;
            }

            // -- Multi-track messages --
            Message::AddTrack => {
                let track_num = self.state.next_track_number;
                let color_index = (track_num.wrapping_sub(1) % 8) as u8;
                self.state.next_track_number += 1;
                let id = TrackId::new();
                let name = format!("Track {track_num}");

                self.send_command(EngineCommand::AddTrack(id, name.clone()));
                self.state.tracks.push(UiTrack::new(id, name, color_index));
                self.state.selected_track = Some(id);
                self.state.status_text = format!("{} tracks", self.state.tracks.len());
            }
            Message::RemoveTrack(track_id) => {
                self.send_command(EngineCommand::RemoveTrack(track_id));
                self.state.tracks.retain(|t| t.id != track_id);
                if self.state.selected_track == Some(track_id) {
                    self.state.selected_track = self.state.tracks.first().map(|t| t.id);
                }
                // Clear note clip selection if track removed
                if let Some((tid, _)) = self.state.selected_note_clip {
                    if tid == track_id {
                        self.state.selected_note_clip = None;
                    }
                }
                // Clear arrangement selection if track removed
                if let Some(sel) = self.state.selected_arrangement_clip {
                    let sel_track = match sel {
                        ArrangementSelection::AudioClip { track_id: t, .. } => t,
                        ArrangementSelection::NoteClip { track_id: t, .. } => t,
                    };
                    if sel_track == track_id {
                        self.state.selected_arrangement_clip = None;
                    }
                }
                if self.state.tracks.is_empty() {
                    self.state.status_text = "Ready — Add a track to get started".to_string();
                } else {
                    self.state.status_text = format!("{} tracks", self.state.tracks.len());
                }
            }
            Message::SelectTrack(track_id) => {
                self.state.selected_track = Some(track_id);
            }
            Message::AddClipToTrack(track_id) => {
                // Guard: only audio tracks can have audio clips
                if let Some(track) = self.state.find_track(track_id) {
                    if matches!(track.kind, TrackKind::Instrument(_)) {
                        self.state.status_text =
                            "Instrument tracks use note clips, not audio".to_string();
                        return Task::none();
                    }
                }
                return Task::perform(
                    async {
                        let handle = rfd::AsyncFileDialog::new()
                            .set_title("Add Audio Clip")
                            .add_filter("Audio", &["wav", "mp3", "flac", "ogg"])
                            .pick_file()
                            .await;
                        handle.map(|h| h.path().to_path_buf())
                    },
                    move |path| Message::ClipFileSelected(track_id, path),
                );
            }
            Message::ClipFileSelected(track_id, path) => {
                if let Some(path) = path {
                    let file_name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    self.state.status_text = format!("Loading {file_name}...");
                    let clip_id = ClipId::new();

                    return Task::perform(decode_file_async(path), move |result| match result {
                        Ok(audio) => Message::ClipAudioDecoded(
                            track_id,
                            clip_id,
                            Arc::new(audio),
                            file_name.clone(),
                        ),
                        Err(e) => Message::ClipDecodeError(track_id, e),
                    });
                }
            }
            Message::ClipAudioDecoded(track_id, clip_id, audio, name) => {
                let existing_end = self
                    .state
                    .find_track(track_id)
                    .map(|t| {
                        t.clips
                            .iter()
                            .map(|c| c.position.saturating_add(c.duration))
                            .max()
                            .unwrap_or(0)
                    })
                    .unwrap_or(0);

                let duration = audio.num_frames() as u64;

                self.send_command(EngineCommand::AddClip {
                    track_id,
                    clip_id,
                    audio: Arc::clone(&audio),
                    position: existing_end,
                    source_offset: 0,
                    duration,
                    loop_enabled: false,
                    loop_start: 0,
                    loop_end: 0,
                });

                if let Some(track) = self.state.find_track_mut(track_id) {
                    track.clips.push(UiClip {
                        id: clip_id,
                        name: name.clone(),
                        audio,
                        position: existing_end,
                        source_offset: 0,
                        duration,
                        loop_enabled: false,
                        loop_start: 0,
                        loop_end: 0,
                    });
                }

                self.state.status_text = format!("Added clip: {name}");
            }
            Message::ClipDecodeError(_, err) => {
                self.state.status_text = format!("Error: {err}");
            }
            Message::RemoveClip(track_id, clip_id) => {
                self.send_command(EngineCommand::RemoveClip(track_id, clip_id));
                if let Some(track) = self.state.find_track_mut(track_id) {
                    track.clips.retain(|c| c.id != clip_id);
                }
                // Clear arrangement selection if this clip was selected
                if self.state.selected_arrangement_clip
                    == Some(ArrangementSelection::AudioClip { track_id, clip_id })
                {
                    self.state.selected_arrangement_clip = None;
                }
            }
            Message::SetTrackGain(track_id, gain) => {
                let gain = gain.clamp(0.0, 2.0);
                self.send_command(EngineCommand::SetTrackGain(track_id, gain));
                if let Some(track) = self.state.find_track_mut(track_id) {
                    track.gain = gain;
                }
            }
            Message::SetTrackPan(track_id, pan) => {
                let pan = pan.clamp(0.0, 1.0);
                self.send_command(EngineCommand::SetTrackPan(track_id, pan));
                if let Some(track) = self.state.find_track_mut(track_id) {
                    track.pan = pan;
                }
            }
            Message::SetTrackMute(track_id) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    track.mute = !track.mute;
                    let mute = track.mute;
                    self.send_command(EngineCommand::SetTrackMute(track_id, mute));
                }
            }
            Message::SetTrackSolo(track_id) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    track.solo = !track.solo;
                    let solo = track.solo;
                    self.send_command(EngineCommand::SetTrackSolo(track_id, solo));
                }
            }
            Message::EngineTrackMeter {
                track_id,
                peak_l,
                peak_r,
            } => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    track.peak_l = peak_l.max(track.peak_l * 0.85);
                    track.peak_r = peak_r.max(track.peak_r * 0.85);
                }
            }

            // -- Effects --
            Message::AddEffect(track_id, effect_type) => {
                let effect_id = vibez_core::id::EffectId::new();
                let fx =
                    vibez_dsp::factory::create_effect(effect_type, self.state.sample_rate as f32);
                let descriptors = fx.param_descriptors();
                let params: Vec<f32> = descriptors.iter().map(|d| d.default).collect();

                if let Some(track) = self.state.find_track_mut(track_id) {
                    track.effects.push(UiEffect {
                        id: effect_id,
                        effect_type,
                        bypass: false,
                        params,
                        descriptors,
                    });
                }
                self.send_command(EngineCommand::AddEffect {
                    track_id,
                    effect_id,
                    effect_type,
                    position: None,
                });
                self.state.status_text = format!("Added {} effect", effect_type.name());
            }
            Message::RemoveEffect(track_id, effect_id) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    track.effects.retain(|e| e.id != effect_id);
                }
                self.send_command(EngineCommand::RemoveEffect(track_id, effect_id));
                self.state.status_text = "Removed effect".to_string();
            }
            Message::SetEffectParam(track_id, effect_id, param_index, value) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(effect) = track.effects.iter_mut().find(|e| e.id == effect_id) {
                        if param_index < effect.params.len() {
                            let desc = &effect.descriptors[param_index];
                            let clamped = value.clamp(desc.min, desc.max);
                            effect.params[param_index] = clamped;
                            self.send_command(EngineCommand::SetEffectParam {
                                track_id,
                                effect_id,
                                param_index,
                                value: clamped,
                            });
                        }
                    }
                }
            }
            Message::ToggleEffectBypass(track_id, effect_id) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(effect) = track.effects.iter_mut().find(|e| e.id == effect_id) {
                        effect.bypass = !effect.bypass;
                        let bypass = effect.bypass;
                        self.send_command(EngineCommand::SetEffectBypass {
                            track_id,
                            effect_id,
                            bypass,
                        });
                    }
                }
            }
            Message::MoveEffectUp(track_id, effect_id) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(idx) = track.effects.iter().position(|e| e.id == effect_id) {
                        if idx > 0 {
                            track.effects.swap(idx, idx - 1);
                            self.send_command(EngineCommand::MoveEffect {
                                track_id,
                                effect_id,
                                new_index: idx - 1,
                            });
                        }
                    }
                }
            }
            Message::MoveEffectDown(track_id, effect_id) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(idx) = track.effects.iter().position(|e| e.id == effect_id) {
                        if idx + 1 < track.effects.len() {
                            track.effects.swap(idx, idx + 1);
                            self.send_command(EngineCommand::MoveEffect {
                                track_id,
                                effect_id,
                                new_index: idx + 1,
                            });
                        }
                    }
                }
            }

            // -- Instrument tracks --
            Message::AddInstrumentTrack => {
                let track_num = self.state.next_track_number;
                let color_index = (track_num.wrapping_sub(1) % 8) as u8;
                self.state.next_track_number += 1;
                let id = TrackId::new();
                let name = format!("Synth {track_num}");
                let kind = TrackKind::Instrument(InstrumentKind::SubtractiveSynth);

                self.send_command(EngineCommand::AddInstrumentTrack(
                    id,
                    name.clone(),
                    InstrumentKind::SubtractiveSynth,
                ));
                self.state
                    .tracks
                    .push(UiTrack::new_instrument(id, name, kind, color_index));
                self.state.selected_track = Some(id);
                self.state.status_text = format!("{} tracks", self.state.tracks.len());
            }
            Message::SetSynthParam(track_id, param_index, value) => {
                self.send_command(EngineCommand::SetSynthParam {
                    track_id,
                    param_index,
                    value,
                });
                self.state.status_text = format!("Synth param {param_index} = {value:.2}");
            }

            // -- Clip looping --
            Message::ToggleClipLoop(track_id, clip_id) => {
                let mut cmd_data = None;
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.loop_enabled = !clip.loop_enabled;
                        if clip.loop_enabled && clip.loop_end == 0 {
                            clip.loop_start = clip.source_offset;
                            clip.loop_end = clip.source_offset + clip.duration;
                        }
                        cmd_data = Some((clip.loop_enabled, clip.loop_start, clip.loop_end));
                    }
                }
                if let Some((enabled, loop_start, loop_end)) = cmd_data {
                    self.send_command(EngineCommand::SetClipLoop {
                        track_id,
                        clip_id,
                        enabled,
                        loop_start,
                        loop_end,
                    });
                }
            }
            Message::SetClipLoopRegion {
                track_id,
                clip_id,
                loop_start,
                loop_end,
            } => {
                let mut enabled = false;
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.loop_start = loop_start;
                        clip.loop_end = loop_end;
                        enabled = clip.loop_enabled;
                    }
                }
                self.send_command(EngineCommand::SetClipLoop {
                    track_id,
                    clip_id,
                    enabled,
                    loop_start,
                    loop_end,
                });
            }
            Message::ToggleNoteClipLoop(track_id, clip_id) => {
                let mut cmd_data = None;
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.loop_enabled = !clip.loop_enabled;
                        if clip.loop_enabled && clip.loop_end_beats == 0.0 {
                            clip.loop_start_beats = 0.0;
                            clip.loop_end_beats = clip.duration_beats;
                        }
                        cmd_data = Some((
                            clip.loop_enabled,
                            clip.loop_start_beats,
                            clip.loop_end_beats,
                        ));
                    }
                }
                if let Some((enabled, loop_start_beats, loop_end_beats)) = cmd_data {
                    self.send_command(EngineCommand::SetNoteClipLoop {
                        track_id,
                        clip_id,
                        enabled,
                        loop_start_beats,
                        loop_end_beats,
                    });
                }
            }
            Message::SetNoteClipLoopRegion {
                track_id,
                clip_id,
                loop_start_beats,
                loop_end_beats,
            } => {
                let mut enabled = false;
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.loop_start_beats = loop_start_beats;
                        clip.loop_end_beats = loop_end_beats;
                        enabled = clip.loop_enabled;
                    }
                }
                self.send_command(EngineCommand::SetNoteClipLoop {
                    track_id,
                    clip_id,
                    enabled,
                    loop_start_beats,
                    loop_end_beats,
                });
            }

            // -- Piano roll / note clips --
            Message::AddNoteClipToTrack(track_id) => {
                let clip_id = ClipId::new();
                let position_beats = 0.0;
                let duration_beats = 16.0;
                if let Some(track) = self.state.find_track_mut(track_id) {
                    track.note_clips.push(UiNoteClip {
                        id: clip_id,
                        name: format!("Pattern {}", track.note_clips.len() + 1),
                        position_beats,
                        duration_beats,
                        notes: Vec::new(),
                        selected_note: None,
                        loop_enabled: false,
                        loop_start_beats: 0.0,
                        loop_end_beats: 0.0,
                    });
                }
                self.send_command(EngineCommand::AddNoteClip {
                    track_id,
                    clip_id,
                    position_beats,
                    duration_beats,
                    loop_enabled: false,
                    loop_start_beats: 0.0,
                    loop_end_beats: 0.0,
                });
                // Auto-select the new note clip for piano roll editing
                self.state.selected_note_clip = Some((track_id, clip_id));
                self.state.status_text = "Added note clip".to_string();
            }
            Message::SelectNoteClip(track_id, clip_id) => {
                self.state.selected_note_clip = Some((track_id, clip_id));
            }
            Message::AddNote {
                track_id,
                clip_id,
                pitch,
                start_beat,
                duration_beats,
            } => {
                let note = MidiNote {
                    pitch,
                    velocity: 100,
                    start_beat,
                    duration_beats,
                };
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.notes.push(note);
                    }
                }
                self.send_command(EngineCommand::AddNote {
                    track_id,
                    clip_id,
                    note,
                });
            }
            Message::RemoveNote(track_id, clip_id, note_index) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        if note_index < clip.notes.len() {
                            clip.notes.remove(note_index);
                            clip.selected_note = None;
                        }
                    }
                }
                self.send_command(EngineCommand::RemoveNote {
                    track_id,
                    clip_id,
                    note_index,
                });
            }
            Message::EditNote(track_id, clip_id, note_index, new_note) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        if note_index < clip.notes.len() {
                            clip.notes[note_index] = new_note;
                        }
                    }
                }
                self.send_command(EngineCommand::EditNote {
                    track_id,
                    clip_id,
                    note_index,
                    note: new_note,
                });
            }
            Message::SelectNote(track_id, clip_id, note_index) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.selected_note = note_index;
                    }
                }
            }

            // -- Clip operations --
            Message::DuplicateNoteClip(track_id, clip_id) => {
                let new_clip_id = ClipId::new();
                let mut new_clip_data = None;

                if let Some(track) = self.state.find_track(track_id) {
                    if let Some(clip) = track.note_clips.iter().find(|c| c.id == clip_id) {
                        let new_pos = clip.position_beats + clip.duration_beats;
                        new_clip_data = Some((
                            UiNoteClip {
                                id: new_clip_id,
                                name: format!("{} (copy)", clip.name),
                                position_beats: new_pos,
                                duration_beats: clip.duration_beats,
                                notes: clip.notes.clone(),
                                selected_note: None,
                                loop_enabled: clip.loop_enabled,
                                loop_start_beats: clip.loop_start_beats,
                                loop_end_beats: clip.loop_end_beats,
                            },
                            new_pos,
                            clip.duration_beats,
                            clip.notes.clone(),
                        ));
                    }
                }

                if let Some((new_clip, pos, dur, notes)) = new_clip_data {
                    if let Some(track) = self.state.find_track_mut(track_id) {
                        track.note_clips.push(new_clip);
                    }
                    self.send_command(EngineCommand::AddNoteClip {
                        track_id,
                        clip_id: new_clip_id,
                        position_beats: pos,
                        duration_beats: dur,
                        loop_enabled: false,
                        loop_start_beats: 0.0,
                        loop_end_beats: 0.0,
                    });
                    for note in &notes {
                        self.send_command(EngineCommand::AddNote {
                            track_id,
                            clip_id: new_clip_id,
                            note: *note,
                        });
                    }
                    self.state.selected_note_clip = Some((track_id, new_clip_id));
                    self.state.status_text = "Duplicated clip".to_string();
                }
            }
            Message::DoubleNoteClip(track_id, clip_id) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        let orig_dur = clip.duration_beats;
                        let cloned_notes: Vec<MidiNote> = clip
                            .notes
                            .iter()
                            .map(|n| MidiNote {
                                start_beat: n.start_beat + orig_dur,
                                ..*n
                            })
                            .collect();
                        clip.notes.extend_from_slice(&cloned_notes);
                        clip.duration_beats *= 2.0;

                        // Send new notes to engine
                        for note in &cloned_notes {
                            self.send_command(EngineCommand::AddNote {
                                track_id,
                                clip_id,
                                note: *note,
                            });
                        }
                    }
                }
                self.state.status_text = "Doubled clip length".to_string();
            }
            Message::CropNoteClip(track_id, clip_id) => {
                let mut sync_data = None;
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        if !clip.notes.is_empty() {
                            let min_beat = clip
                                .notes
                                .iter()
                                .map(|n| n.start_beat)
                                .fold(f64::INFINITY, f64::min);
                            let max_beat = clip
                                .notes
                                .iter()
                                .map(|n| n.start_beat + n.duration_beats)
                                .fold(f64::NEG_INFINITY, f64::max);

                            // Shift notes so first note starts at 0
                            for note in &mut clip.notes {
                                note.start_beat -= min_beat;
                            }
                            clip.position_beats += min_beat;
                            clip.duration_beats = max_beat - min_beat;

                            sync_data = Some((
                                clip.position_beats,
                                clip.duration_beats,
                                clip.notes.clone(),
                            ));
                        }
                    }
                }
                // Sync to engine outside the mutable borrow
                if let Some((pos, dur, notes)) = sync_data {
                    self.send_command(EngineCommand::RemoveNoteClip(track_id, clip_id));
                    self.send_command(EngineCommand::AddNoteClip {
                        track_id,
                        clip_id,
                        position_beats: pos,
                        duration_beats: dur,
                        loop_enabled: false,
                        loop_start_beats: 0.0,
                        loop_end_beats: 0.0,
                    });
                    for note in &notes {
                        self.send_command(EngineCommand::AddNote {
                            track_id,
                            clip_id,
                            note: *note,
                        });
                    }
                }
                self.state.status_text = "Cropped clip to content".to_string();
            }

            // -- Piano roll scroll --
            Message::PianoRollScrollY(y) => {
                self.state.piano_roll_scroll_y = y;
            }

            // ── Arrangement clip interaction ──
            Message::SelectArrangementClip(selection) => {
                self.state.selected_arrangement_clip = Some(selection);
                self.state.detail_panel_tab = DetailPanelTab::Clip;
                // Also update track selection and note clip selection for detail panel
                match selection {
                    ArrangementSelection::AudioClip { track_id, .. } => {
                        self.state.selected_track = Some(track_id);
                        // Clear note clip selection when an audio clip is selected
                        self.state.selected_note_clip = None;
                    }
                    ArrangementSelection::NoteClip { track_id, clip_id } => {
                        self.state.selected_track = Some(track_id);
                        self.state.selected_note_clip = Some((track_id, clip_id));
                    }
                }
            }

            Message::MoveAudioClip {
                track_id,
                clip_id,
                new_position,
            } => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.position = new_position;
                    }
                }
                self.send_command(EngineCommand::MoveClip {
                    track_id,
                    clip_id,
                    new_position,
                });
            }

            Message::MoveNoteClipPosition {
                track_id,
                clip_id,
                new_position_beats,
            } => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.position_beats = new_position_beats;
                    }
                }
                self.send_command(EngineCommand::MoveNoteClip {
                    track_id,
                    clip_id,
                    new_position_beats,
                });
            }

            Message::ResizeAudioClip {
                track_id,
                clip_id,
                new_duration,
            } => {
                // Update UI state — auto-enable loop when extending past source length
                let mut sync_data = None;
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                        let source_len = clip.audio.num_frames() as u64 - clip.source_offset;
                        if new_duration > source_len {
                            // Extending past source: enable loop over full source region
                            clip.duration = new_duration;
                            if !clip.loop_enabled {
                                clip.loop_enabled = true;
                                clip.loop_start = clip.source_offset;
                                clip.loop_end = clip.source_offset + source_len;
                            }
                        } else {
                            clip.duration = new_duration;
                        }
                        sync_data = Some((
                            Arc::clone(&clip.audio),
                            clip.position,
                            clip.source_offset,
                            clip.duration,
                            clip.loop_enabled,
                            clip.loop_start,
                            clip.loop_end,
                        ));
                    }
                }
                // Sync to engine via Remove+Add (loop state included atomically)
                if let Some((
                    audio,
                    position,
                    source_offset,
                    duration,
                    loop_enabled,
                    loop_start,
                    loop_end,
                )) = sync_data
                {
                    self.send_command(EngineCommand::RemoveClip(track_id, clip_id));
                    self.send_command(EngineCommand::AddClip {
                        track_id,
                        clip_id,
                        audio,
                        position,
                        source_offset,
                        duration,
                        loop_enabled,
                        loop_start,
                        loop_end,
                    });
                }
            }

            Message::ResizeNoteClipDuration {
                track_id,
                clip_id,
                new_duration_beats,
            } => {
                let mut sync_data = None;
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        // Determine content length from note extent
                        let content_end = clip
                            .notes
                            .iter()
                            .map(|n| n.start_beat + n.duration_beats)
                            .fold(0.0_f64, f64::max)
                            .max(clip.loop_end_beats);
                        // Use the old duration as the content boundary if no notes
                        let content_len = if content_end > 0.0 {
                            content_end
                        } else {
                            clip.duration_beats
                        };

                        clip.duration_beats = new_duration_beats;

                        // Auto-enable loop when extending past content
                        if new_duration_beats > content_len && !clip.loop_enabled {
                            clip.loop_enabled = true;
                            clip.loop_start_beats = 0.0;
                            clip.loop_end_beats = content_len;
                        }

                        sync_data = Some((
                            clip.position_beats,
                            clip.duration_beats,
                            clip.notes.clone(),
                            clip.loop_enabled,
                            clip.loop_start_beats,
                            clip.loop_end_beats,
                        ));
                    }
                }
                // Sync to engine via Remove+Add+re-add-notes (loop state included atomically)
                if let Some((pos, dur, notes, loop_enabled, loop_start_beats, loop_end_beats)) =
                    sync_data
                {
                    self.send_command(EngineCommand::RemoveNoteClip(track_id, clip_id));
                    self.send_command(EngineCommand::AddNoteClip {
                        track_id,
                        clip_id,
                        position_beats: pos,
                        duration_beats: dur,
                        loop_enabled,
                        loop_start_beats,
                        loop_end_beats,
                    });
                    for note in &notes {
                        self.send_command(EngineCommand::AddNote {
                            track_id,
                            clip_id,
                            note: *note,
                        });
                    }
                }
            }

            Message::MoveClipToTrack {
                source_track,
                target_track,
                clip_id,
                is_note_clip,
            } => {
                if is_note_clip {
                    // Move note clip between instrument tracks
                    let mut clip_data = None;
                    if let Some(track) = self.state.find_track_mut(source_track) {
                        if let Some(idx) = track.note_clips.iter().position(|c| c.id == clip_id) {
                            clip_data = Some(track.note_clips.remove(idx));
                        }
                    }
                    if let Some(clip) = clip_data {
                        // Remove from engine source track
                        self.send_command(EngineCommand::RemoveNoteClip(source_track, clip_id));
                        // Add to engine target track
                        self.send_command(EngineCommand::AddNoteClip {
                            track_id: target_track,
                            clip_id,
                            position_beats: clip.position_beats,
                            duration_beats: clip.duration_beats,
                            loop_enabled: clip.loop_enabled,
                            loop_start_beats: clip.loop_start_beats,
                            loop_end_beats: clip.loop_end_beats,
                        });
                        for note in &clip.notes {
                            self.send_command(EngineCommand::AddNote {
                                track_id: target_track,
                                clip_id,
                                note: *note,
                            });
                        }
                        // Add to UI target track
                        if let Some(track) = self.state.find_track_mut(target_track) {
                            track.note_clips.push(clip);
                        }
                        // Update selection
                        self.state.selected_arrangement_clip =
                            Some(ArrangementSelection::NoteClip {
                                track_id: target_track,
                                clip_id,
                            });
                        self.state.selected_track = Some(target_track);
                        self.state.selected_note_clip = Some((target_track, clip_id));
                    }
                } else {
                    // Move audio clip between audio tracks
                    let mut clip_data = None;
                    if let Some(track) = self.state.find_track_mut(source_track) {
                        if let Some(idx) = track.clips.iter().position(|c| c.id == clip_id) {
                            clip_data = Some(track.clips.remove(idx));
                        }
                    }
                    if let Some(clip) = clip_data {
                        // Remove from engine source track
                        self.send_command(EngineCommand::RemoveClip(source_track, clip_id));
                        // Add to engine target track
                        self.send_command(EngineCommand::AddClip {
                            track_id: target_track,
                            clip_id,
                            audio: Arc::clone(&clip.audio),
                            position: clip.position,
                            source_offset: clip.source_offset,
                            duration: clip.duration,
                            loop_enabled: clip.loop_enabled,
                            loop_start: clip.loop_start,
                            loop_end: clip.loop_end,
                        });
                        // Add to UI target track
                        if let Some(track) = self.state.find_track_mut(target_track) {
                            track.clips.push(clip);
                        }
                        // Update selection
                        self.state.selected_arrangement_clip =
                            Some(ArrangementSelection::AudioClip {
                                track_id: target_track,
                                clip_id,
                            });
                        self.state.selected_track = Some(target_track);
                    }
                }
            }

            Message::SplitAudioClip {
                track_id,
                clip_id,
                split_position,
            } => {
                let mut split_data = None;
                if let Some(track) = self.state.find_track(track_id) {
                    if let Some(clip) = track.clips.iter().find(|c| c.id == clip_id) {
                        if split_position > clip.position
                            && split_position < clip.position + clip.duration
                        {
                            let left_dur = split_position - clip.position;
                            let right_dur = clip.duration - left_dur;
                            let right_source_offset = clip.source_offset + left_dur;
                            split_data = Some((
                                Arc::clone(&clip.audio),
                                clip.name.clone(),
                                clip.position,
                                clip.source_offset,
                                left_dur,
                                split_position,
                                right_source_offset,
                                right_dur,
                            ));
                        }
                    }
                }
                if let Some((
                    audio,
                    name,
                    orig_pos,
                    orig_offset,
                    left_dur,
                    right_pos,
                    right_offset,
                    right_dur,
                )) = split_data
                {
                    let left_id = ClipId::new();
                    let right_id = ClipId::new();

                    // Remove original
                    self.send_command(EngineCommand::RemoveClip(track_id, clip_id));
                    if let Some(track) = self.state.find_track_mut(track_id) {
                        track.clips.retain(|c| c.id != clip_id);
                    }

                    // Add left half
                    self.send_command(EngineCommand::AddClip {
                        track_id,
                        clip_id: left_id,
                        audio: Arc::clone(&audio),
                        position: orig_pos,
                        source_offset: orig_offset,
                        duration: left_dur,
                        loop_enabled: false,
                        loop_start: 0,
                        loop_end: 0,
                    });
                    if let Some(track) = self.state.find_track_mut(track_id) {
                        track.clips.push(UiClip {
                            id: left_id,
                            name: format!("{name} L"),
                            audio: Arc::clone(&audio),
                            position: orig_pos,
                            source_offset: orig_offset,
                            duration: left_dur,
                            loop_enabled: false,
                            loop_start: 0,
                            loop_end: 0,
                        });
                    }

                    // Add right half
                    self.send_command(EngineCommand::AddClip {
                        track_id,
                        clip_id: right_id,
                        audio: Arc::clone(&audio),
                        position: right_pos,
                        source_offset: right_offset,
                        duration: right_dur,
                        loop_enabled: false,
                        loop_start: 0,
                        loop_end: 0,
                    });
                    if let Some(track) = self.state.find_track_mut(track_id) {
                        track.clips.push(UiClip {
                            id: right_id,
                            name: format!("{name} R"),
                            audio,
                            position: right_pos,
                            source_offset: right_offset,
                            duration: right_dur,
                            loop_enabled: false,
                            loop_start: 0,
                            loop_end: 0,
                        });
                    }

                    // Select left half
                    self.state.selected_arrangement_clip = Some(ArrangementSelection::AudioClip {
                        track_id,
                        clip_id: left_id,
                    });
                    self.state.status_text = "Split audio clip".to_string();
                }
            }

            Message::SplitNoteClip {
                track_id,
                clip_id,
                split_beat,
            } => {
                let mut split_data = None;
                if let Some(track) = self.state.find_track(track_id) {
                    if let Some(clip) = track.note_clips.iter().find(|c| c.id == clip_id) {
                        let clip_end = clip.position_beats + clip.duration_beats;
                        if split_beat > clip.position_beats && split_beat < clip_end {
                            let local_split = split_beat - clip.position_beats;
                            let left_dur = local_split;
                            let right_dur = clip.duration_beats - local_split;

                            let mut left_notes = Vec::new();
                            let mut right_notes = Vec::new();
                            for note in &clip.notes {
                                if note.start_beat < local_split {
                                    left_notes.push(*note);
                                } else {
                                    right_notes.push(MidiNote {
                                        start_beat: note.start_beat - local_split,
                                        ..*note
                                    });
                                }
                            }

                            split_data = Some((
                                clip.name.clone(),
                                clip.position_beats,
                                left_dur,
                                split_beat,
                                right_dur,
                                left_notes,
                                right_notes,
                            ));
                        }
                    }
                }
                if let Some((
                    name,
                    orig_pos,
                    left_dur,
                    right_pos,
                    right_dur,
                    left_notes,
                    right_notes,
                )) = split_data
                {
                    let left_id = ClipId::new();
                    let right_id = ClipId::new();

                    // Remove original
                    self.send_command(EngineCommand::RemoveNoteClip(track_id, clip_id));
                    if let Some(track) = self.state.find_track_mut(track_id) {
                        track.note_clips.retain(|c| c.id != clip_id);
                    }

                    // Add left half
                    self.send_command(EngineCommand::AddNoteClip {
                        track_id,
                        clip_id: left_id,
                        position_beats: orig_pos,
                        duration_beats: left_dur,
                        loop_enabled: false,
                        loop_start_beats: 0.0,
                        loop_end_beats: 0.0,
                    });
                    for note in &left_notes {
                        self.send_command(EngineCommand::AddNote {
                            track_id,
                            clip_id: left_id,
                            note: *note,
                        });
                    }
                    if let Some(track) = self.state.find_track_mut(track_id) {
                        track.note_clips.push(UiNoteClip {
                            id: left_id,
                            name: format!("{name} L"),
                            position_beats: orig_pos,
                            duration_beats: left_dur,
                            notes: left_notes,
                            selected_note: None,
                            loop_enabled: false,
                            loop_start_beats: 0.0,
                            loop_end_beats: 0.0,
                        });
                    }

                    // Add right half
                    self.send_command(EngineCommand::AddNoteClip {
                        track_id,
                        clip_id: right_id,
                        position_beats: right_pos,
                        duration_beats: right_dur,
                        loop_enabled: false,
                        loop_start_beats: 0.0,
                        loop_end_beats: 0.0,
                    });
                    for note in &right_notes {
                        self.send_command(EngineCommand::AddNote {
                            track_id,
                            clip_id: right_id,
                            note: *note,
                        });
                    }
                    if let Some(track) = self.state.find_track_mut(track_id) {
                        track.note_clips.push(UiNoteClip {
                            id: right_id,
                            name: format!("{name} R"),
                            position_beats: right_pos,
                            duration_beats: right_dur,
                            notes: right_notes,
                            selected_note: None,
                            loop_enabled: false,
                            loop_start_beats: 0.0,
                            loop_end_beats: 0.0,
                        });
                    }

                    self.state.selected_arrangement_clip = Some(ArrangementSelection::NoteClip {
                        track_id,
                        clip_id: left_id,
                    });
                    self.state.selected_note_clip = Some((track_id, left_id));
                    self.state.status_text = "Split note clip".to_string();
                }
            }

            Message::DeleteSelectedClip => {
                if let Some(selection) = self.state.selected_arrangement_clip.take() {
                    match selection {
                        ArrangementSelection::AudioClip { track_id, clip_id } => {
                            self.send_command(EngineCommand::RemoveClip(track_id, clip_id));
                            if let Some(track) = self.state.find_track_mut(track_id) {
                                track.clips.retain(|c| c.id != clip_id);
                            }
                        }
                        ArrangementSelection::NoteClip { track_id, clip_id } => {
                            self.send_command(EngineCommand::RemoveNoteClip(track_id, clip_id));
                            if let Some(track) = self.state.find_track_mut(track_id) {
                                track.note_clips.retain(|c| c.id != clip_id);
                            }
                            if self
                                .state
                                .selected_note_clip
                                .is_some_and(|(tid, cid)| tid == track_id && cid == clip_id)
                            {
                                self.state.selected_note_clip = None;
                            }
                        }
                    }
                    self.state.status_text = "Deleted clip".to_string();
                }
            }

            Message::DuplicateSelectedClip => {
                if let Some(selection) = self.state.selected_arrangement_clip {
                    match selection {
                        ArrangementSelection::AudioClip { track_id, clip_id } => {
                            let mut dup_data = None;
                            if let Some(track) = self.state.find_track(track_id) {
                                if let Some(clip) = track.clips.iter().find(|c| c.id == clip_id) {
                                    let new_pos = clip.position + clip.duration;
                                    dup_data = Some((
                                        Arc::clone(&clip.audio),
                                        clip.name.clone(),
                                        new_pos,
                                        clip.source_offset,
                                        clip.duration,
                                    ));
                                }
                            }
                            if let Some((audio, name, position, source_offset, duration)) = dup_data
                            {
                                let new_id = ClipId::new();
                                self.send_command(EngineCommand::AddClip {
                                    track_id,
                                    clip_id: new_id,
                                    audio: Arc::clone(&audio),
                                    position,
                                    source_offset,
                                    duration,
                                    loop_enabled: false,
                                    loop_start: 0,
                                    loop_end: 0,
                                });
                                if let Some(track) = self.state.find_track_mut(track_id) {
                                    track.clips.push(UiClip {
                                        id: new_id,
                                        name: format!("{name} (copy)"),
                                        audio,
                                        position,
                                        source_offset,
                                        duration,
                                        loop_enabled: false,
                                        loop_start: 0,
                                        loop_end: 0,
                                    });
                                }
                                self.state.selected_arrangement_clip =
                                    Some(ArrangementSelection::AudioClip {
                                        track_id,
                                        clip_id: new_id,
                                    });
                                self.state.status_text = "Duplicated audio clip".to_string();
                            }
                        }
                        ArrangementSelection::NoteClip { track_id, clip_id } => {
                            // Reuse existing DuplicateNoteClip logic
                            return self.update(Message::DuplicateNoteClip(track_id, clip_id));
                        }
                    }
                }
            }
        }
        Task::none()
    }

    fn poll_engine_events(&mut self) {
        if let Some(ref mut rx) = self.event_rx {
            while let Ok(event) = rx.pop() {
                match event {
                    EngineEvent::PlaybackPosition(pos) => {
                        self.state.position_samples = pos;
                    }
                    EngineEvent::Metering { peak_l, peak_r, .. } => {
                        self.state.peak_l = peak_l.max(self.state.peak_l * 0.85);
                        self.state.peak_r = peak_r.max(self.state.peak_r * 0.85);
                    }
                    EngineEvent::PlaybackStopped => {
                        self.state.playing = false;
                    }
                    EngineEvent::PlaybackStarted => {
                        self.state.playing = true;
                    }
                    EngineEvent::TrackMeter {
                        track_id,
                        peak_l,
                        peak_r,
                    } => {
                        if let Some(track) = self.state.find_track_mut(track_id) {
                            track.peak_l = peak_l.max(track.peak_l * 0.85);
                            track.peak_r = peak_r.max(track.peak_r * 0.85);
                        }
                    }
                }
            }
        }
    }

    // ── View ──

    fn view(&self) -> Element<'_, Message> {
        let header = self.view_header();

        let content = match self.state.workspace {
            Workspace::Arrange => self.view_arrangement(),
            Workspace::Mix => self.view_mixer(),
        };

        let detail_panel = self.view_detail_panel();
        let transport_bar = self.view_transport();
        let status_bar = self.view_status();

        let layout = column![header, content, detail_panel, transport_bar, status_bar];

        container(layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn view_header(&self) -> Element<'_, Message> {
        let title = text("vibez").size(22).color(th::ACCENT);

        // Workspace tabs
        let arrange_tab = {
            let active = self.state.workspace == Workspace::Arrange;
            let (bg, text_color, border_color) = if active {
                (th::BG_ELEVATED, th::ACCENT, th::ACCENT_DIM)
            } else {
                (
                    iced::Color::TRANSPARENT,
                    th::TEXT_DIM,
                    iced::Color::TRANSPARENT,
                )
            };
            button(
                row![
                    icons::icon(icons::LAYOUT_LIST).size(13).color(text_color),
                    text("Arrange").size(13).color(text_color)
                ]
                .spacing(4)
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::SwitchWorkspace(Workspace::Arrange))
            .padding([6, 14])
            .style(move |_theme: &Theme, _status| button::Style {
                background: Some(bg.into()),
                text_color,
                border: iced::Border {
                    color: border_color,
                    width: if active { 1.0 } else { 0.0 },
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
        };

        let mix_tab = {
            let active = self.state.workspace == Workspace::Mix;
            let (bg, text_color, border_color) = if active {
                (th::BG_ELEVATED, th::ACCENT, th::ACCENT_DIM)
            } else {
                (
                    iced::Color::TRANSPARENT,
                    th::TEXT_DIM,
                    iced::Color::TRANSPARENT,
                )
            };
            button(
                row![
                    icons::icon(icons::SLIDERS_VERTICAL)
                        .size(13)
                        .color(text_color),
                    text("Mix").size(13).color(text_color)
                ]
                .spacing(4)
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::SwitchWorkspace(Workspace::Mix))
            .padding([6, 14])
            .style(move |_theme: &Theme, _status| button::Style {
                background: Some(bg.into()),
                text_color,
                border: iced::Border {
                    color: border_color,
                    width: if active { 1.0 } else { 0.0 },
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
        };

        let tabs = row![arrange_tab, mix_tab].spacing(4);

        // Add track buttons with icons
        let add_audio_btn = button(
            row![
                icons::icon(icons::AUDIO_WAVEFORM).size(13),
                text("Audio").size(13)
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::AddTrack)
        .padding([6, 14])
        .style(|_theme: &Theme, _status| button::Style {
            background: Some(th::BG_ELEVATED.into()),
            text_color: th::TEXT,
            border: iced::Border {
                color: th::BORDER,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        });

        let add_synth_btn = button(
            row![icons::icon(icons::MUSIC).size(13), text("Synth").size(13)]
                .spacing(4)
                .align_y(iced::Alignment::Center),
        )
        .on_press(Message::AddInstrumentTrack)
        .padding([6, 14])
        .style(|_theme: &Theme, _status| button::Style {
            background: Some(th::BG_ELEVATED.into()),
            text_color: th::TEXT,
            border: iced::Border {
                color: th::BORDER,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        });

        let mut header_row = row![
            title,
            tabs,
            horizontal_space(),
            add_audio_btn,
            add_synth_btn
        ]
        .spacing(8);

        if let Some(selected_id) = self.state.selected_track {
            let remove_btn = button(
                row![
                    icons::icon(icons::TRASH_2).size(13),
                    text("Remove").size(13)
                ]
                .spacing(4)
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::RemoveTrack(selected_id))
            .padding([6, 14])
            .style(|_theme: &Theme, _status| button::Style {
                background: Some(th::BG_ELEVATED.into()),
                text_color: th::DANGER,
                border: iced::Border {
                    color: th::BORDER,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            });
            header_row = header_row.push(remove_btn);
        }

        let header = header_row.padding(10).align_y(iced::Alignment::Center);

        container(header)
            .width(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::BG_SURFACE.into()),
                border: iced::Border {
                    color: th::BORDER,
                    width: 0.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    // ── Arrangement view ──

    fn view_arrangement(&self) -> Element<'_, Message> {
        if self.state.tracks.is_empty() {
            let prompt = text("Add a track to get started")
                .size(16)
                .color(th::TEXT_DIM);

            let centered = center(prompt).width(Length::Fill).height(Length::Fill);

            return container(centered)
                .width(Length::Fill)
                .height(Length::FillPortion(5))
                .style(|_theme: &Theme| container::Style {
                    background: Some(th::BG_DARK.into()),
                    ..Default::default()
                })
                .into();
        }

        let playhead_beats = self.state.position_beats();
        let sample_rate = self.state.sample_rate;
        let bpm = self.state.bpm;
        let zoom_level = self.state.zoom_level;
        let scroll_offset = self.state.scroll_offset_beats;
        let total_beats = self.state.total_beats();

        // Beat-based ruler across the top (offset by track header width)
        let ruler = RulerWidget {
            playhead_beats,
            bpm,
            zoom_level,
            scroll_offset_beats: scroll_offset,
            total_beats,
        };
        let ruler_canvas: Element<'_, Message> = canvas(ruler)
            .width(Length::Fill)
            .height(Length::Fixed(28.0))
            .into();

        // Spacer matching header width (including color bar) for the ruler row
        let ruler_spacer = container(text(""))
            .width(Length::Fixed(
                crate::widgets::track_header::TRACK_HEADER_TOTAL_WIDTH,
            ))
            .height(Length::Fixed(28.0))
            .style(|_theme: &Theme| iced::widget::container::Style {
                background: Some(crate::theme::BG_SURFACE.into()),
                ..Default::default()
            });

        let ruler_row = row![ruler_spacer, ruler_canvas];

        // Collect track IDs and kinds for cross-track drag
        let track_ids: Vec<TrackId> = self.state.tracks.iter().map(|t| t.id).collect();
        let track_kinds: Vec<bool> = self
            .state
            .tracks
            .iter()
            .map(|t| matches!(t.kind, TrackKind::Instrument(_)))
            .collect();
        let total_track_count = self.state.tracks.len();

        // Track rows: header widgets + clip canvas
        let mut track_rows = column![].spacing(0);

        for (track_index, track) in self.state.tracks.iter().enumerate() {
            let selected = self.state.selected_track == Some(track.id);
            let track_color = th::track_color(track.color_index);

            // Determine which clip is selected on this track
            let selected_clip = self
                .state
                .selected_arrangement_clip
                .and_then(|sel| match sel {
                    ArrangementSelection::AudioClip { track_id, clip_id }
                        if track_id == track.id =>
                    {
                        Some(clip_id)
                    }
                    ArrangementSelection::NoteClip { track_id, clip_id }
                        if track_id == track.id =>
                    {
                        Some(clip_id)
                    }
                    _ => None,
                });

            // Track header (iced widgets)
            let header = view_track_header(track);

            // Clip canvas for this track
            let clip_canvas_widget = TrackClipCanvas::from_track(
                track,
                playhead_beats,
                zoom_level,
                scroll_offset,
                total_beats,
                sample_rate,
                selected,
                track_color,
                bpm,
                track.id,
                track_index,
                total_track_count,
                track_ids.clone(),
                track_kinds.clone(),
                selected_clip,
            );
            let clip_canvas: Element<'_, Message> = canvas(clip_canvas_widget)
                .width(Length::Fill)
                .height(Length::Fixed(70.0))
                .into();

            let track_row = row![header, clip_canvas].height(Length::Fixed(70.0));

            track_rows = track_rows.push(track_row);
        }

        let content = column![ruler_row, track_rows];

        let scrollable_content = scrollable(content).direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::default(),
        ));

        container(scrollable_content)
            .width(Length::Fill)
            .height(Length::FillPortion(5))
            .style(|_theme: &Theme| container::Style {
                background: Some(th::BG_DARK.into()),
                ..Default::default()
            })
            .into()
    }

    // ── Mixer view ──

    fn view_mixer(&self) -> Element<'_, Message> {
        if self.state.tracks.is_empty() {
            let prompt = text("Add a track to get started")
                .size(16)
                .color(th::TEXT_DIM);

            let centered = center(prompt).width(Length::Fill).height(Length::Fill);

            return container(centered)
                .width(Length::Fill)
                .height(Length::FillPortion(5))
                .style(|_theme: &Theme| container::Style {
                    background: Some(th::BG_DARK.into()),
                    ..Default::default()
                })
                .into();
        }

        // ── Channel strips + pinned master ──
        let mut strips = row![].spacing(4).padding(8).height(Length::Fill);

        for track in &self.state.tracks {
            let strip = view_mixer_strip(track);
            strips = strips.push(strip);
        }

        // Master strip — pinned to far right
        let master_label = text("Master").size(12).color(th::TEXT);
        let master_meter = VuMeterWidget {
            peak_l: self.state.peak_l,
            peak_r: self.state.peak_r,
        };
        let master_meter_canvas: Element<'_, Message> = canvas(master_meter)
            .width(Length::Fixed(32.0))
            .height(Length::Fill)
            .into();

        let master_col = column![master_label, master_meter_canvas]
            .spacing(4)
            .padding(8)
            .width(Length::Fixed(100.0))
            .height(Length::Fill)
            .align_x(iced::Alignment::Center);

        let master_container =
            container(master_col)
                .height(Length::Fill)
                .style(|_theme: &Theme| container::Style {
                    background: Some(th::BG_ELEVATED.into()),
                    border: iced::Border {
                        color: th::BORDER,
                        width: 1.0,
                        radius: 2.0.into(),
                    },
                    ..Default::default()
                });

        let mixer_row = row![strips, horizontal_space(), master_container]
            .spacing(4)
            .padding([8, 4])
            .height(Length::Fill);

        container(mixer_row)
            .width(Length::Fill)
            .height(Length::FillPortion(5))
            .style(|_theme: &Theme| container::Style {
                background: Some(th::BG_DARK.into()),
                ..Default::default()
            })
            .into()
    }

    // ── Detail panel (Ableton-style device chain) ──

    fn view_detail_panel(&self) -> Element<'_, Message> {
        let detail_content: Element<'_, Message> = if let Some(track) = self
            .state
            .selected_track
            .and_then(|id| self.state.find_track(id))
        {
            let track_id = track.id;
            let track_color = th::track_color(track.color_index);

            // Tab bar
            let clip_tab = {
                let active = self.state.detail_panel_tab == DetailPanelTab::Clip;
                let (bg, text_color, border_color) = if active {
                    (th::BG_ELEVATED, th::ACCENT, th::ACCENT_DIM)
                } else {
                    (
                        iced::Color::TRANSPARENT,
                        th::TEXT_DIM,
                        iced::Color::TRANSPARENT,
                    )
                };
                button(text("Clip").size(12).color(text_color))
                    .on_press(Message::SwitchDetailTab(DetailPanelTab::Clip))
                    .padding([4, 12])
                    .style(move |_theme: &Theme, _status| button::Style {
                        background: Some(bg.into()),
                        text_color,
                        border: iced::Border {
                            color: border_color,
                            width: if active { 1.0 } else { 0.0 },
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    })
            };
            let devices_tab = {
                let active = self.state.detail_panel_tab == DetailPanelTab::Devices;
                let (bg, text_color, border_color) = if active {
                    (th::BG_ELEVATED, th::ACCENT, th::ACCENT_DIM)
                } else {
                    (
                        iced::Color::TRANSPARENT,
                        th::TEXT_DIM,
                        iced::Color::TRANSPARENT,
                    )
                };
                button(text("Devices").size(12).color(text_color))
                    .on_press(Message::SwitchDetailTab(DetailPanelTab::Devices))
                    .padding([4, 12])
                    .style(move |_theme: &Theme, _status| button::Style {
                        background: Some(bg.into()),
                        text_color,
                        border: iced::Border {
                            color: border_color,
                            width: if active { 1.0 } else { 0.0 },
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    })
            };
            let tab_bar = row![clip_tab, devices_tab].spacing(4).padding([4, 8]);

            // Tab content
            let tab_content: Element<'_, Message> = match self.state.detail_panel_tab {
                DetailPanelTab::Clip => {
                    let is_instrument = matches!(track.kind, TrackKind::Instrument(_));
                    // Check for note clip selection on this instrument track
                    let has_note_clip = is_instrument
                        && (matches!(
                            self.state.selected_arrangement_clip,
                            Some(ArrangementSelection::NoteClip { track_id: tid, .. }) if tid == track_id
                        ) || self
                            .state
                            .selected_note_clip
                            .is_some_and(|(tid, _)| tid == track_id));

                    if has_note_clip {
                        self.view_piano_roll_panel(track_id, track_color)
                    } else if let Some(ArrangementSelection::AudioClip {
                        track_id: sel_tid,
                        clip_id: sel_cid,
                    }) = self.state.selected_arrangement_clip
                    {
                        if sel_tid == track_id {
                            if let Some(clip) = track.clips.iter().find(|c| c.id == sel_cid) {
                                self.view_audio_clip_panel(clip, track_color)
                            } else {
                                self.view_clip_placeholder()
                            }
                        } else {
                            self.view_clip_placeholder()
                        }
                    } else {
                        self.view_clip_placeholder()
                    }
                }
                DetailPanelTab::Devices => self.view_device_chain(track_id, track, track_color),
            };

            column![tab_bar, tab_content].height(Length::Fill).into()
        } else {
            let label = text("Select a track to view devices")
                .size(14)
                .color(th::TEXT_DIM);
            center(label)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        };

        container(detail_content)
            .width(Length::Fill)
            .height(Length::FillPortion(2))
            .style(|_theme: &Theme| container::Style {
                background: Some(th::BG_DARK.into()),
                border: iced::Border {
                    color: th::BORDER,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    fn view_clip_placeholder(&self) -> Element<'_, Message> {
        let label = text("Select a clip to view details")
            .size(14)
            .color(th::TEXT_DIM);
        center(label)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Build the device chain for the detail panel.
    fn view_device_chain<'a>(
        &'a self,
        track_id: TrackId,
        track: &'a UiTrack,
        track_color: Color,
    ) -> Element<'a, Message> {
        let is_instrument = matches!(track.kind, TrackKind::Instrument(_));

        // Header: track name + "+ Effect" button
        let track_label = text(format!("{} — Devices", track.name))
            .size(13)
            .color(th::TEXT);

        // "+ Effect" dropdown as buttons
        let mut add_effects_row = row![].spacing(4);
        for &et in EffectType::all() {
            let btn = button(text(et.name()).size(10).color(th::TEXT_DIM))
                .on_press(Message::AddEffect(track_id, et))
                .padding([3, 8])
                .style(|_theme: &Theme, _status| button::Style {
                    background: Some(th::BG_ELEVATED.into()),
                    text_color: th::TEXT_DIM,
                    border: iced::Border {
                        color: th::BORDER,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                });
            add_effects_row = add_effects_row.push(btn);
        }

        let add_effect_label = text("+ Effect").size(11).color(th::ACCENT);
        let header = row![
            track_label,
            horizontal_space(),
            add_effect_label,
            add_effects_row
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);

        // Device cards
        let mut devices_row = row![].spacing(6);

        // Synth device card for instrument tracks
        if is_instrument {
            let synth_card = self.view_synth_device(track_id, track_color);
            devices_row = devices_row.push(synth_card);
        }

        // Effect cards
        for effect in &track.effects {
            let slot = view_effect_slot(track_id, effect, track_color);
            devices_row = devices_row.push(slot);
        }

        let scrollable_devices = scrollable(devices_row).direction(
            scrollable::Direction::Horizontal(scrollable::Scrollbar::default()),
        );

        column![header, scrollable_devices]
            .spacing(6)
            .padding(8)
            .width(Length::Fill)
            .into()
    }

    /// Synth device card for instrument tracks.
    fn view_synth_device(&self, _track_id: TrackId, track_color: Color) -> Element<'_, Message> {
        let dot = text("\u{25CF}").size(10).color(track_color);
        let name = text("Synth").size(11).color(th::TEXT);
        let header = row![dot, name].spacing(4).align_y(iced::Alignment::Center);

        // Basic synth param labels (placeholder knobs)
        let param_names = ["Attack", "Decay", "Sustain", "Release"];
        let mut params_col = column![].spacing(4);
        for param_name in &param_names {
            let label = text(*param_name).size(10).color(th::TEXT_DIM);
            let value = text("0.50").size(9).color(th::TEXT_MUTED);
            let param_row = column![label, value].spacing(1);
            params_col = params_col.push(param_row);
        }

        let card = column![header, params_col]
            .spacing(6)
            .padding(8)
            .width(Length::Fixed(120.0));

        container(card)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::BG_ELEVATED.into()),
                border: iced::Border {
                    color: th::BORDER,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    /// Piano roll panel for the detail panel split view.
    fn view_piano_roll_panel(&self, track_id: TrackId, track_color: Color) -> Element<'_, Message> {
        let playhead_beats = self.state.position_beats();

        let track = self.state.find_track(track_id);
        let selected_clip = self.state.selected_note_clip.and_then(|(tid, cid)| {
            if tid == track_id {
                track.and_then(|t| t.note_clips.iter().find(|c| c.id == cid))
            } else {
                None
            }
        });

        let piano_widget = if let Some(clip) = selected_clip {
            PianoRollWidget::from_clip(
                track_id,
                clip,
                playhead_beats,
                clip.duration_beats,
                track_color,
                self.state.snap_grid,
                self.state.piano_roll_scroll_y,
            )
        } else {
            PianoRollWidget::empty(track_id, playhead_beats, track_color)
        };

        let piano_canvas: Element<'_, Message> = canvas(piano_widget)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();

        let label = text("Piano Roll").size(11).color(th::TEXT_DIM);

        // Clip operation buttons
        let mut clip_ops = row![].spacing(2);
        if let Some((tid, cid)) = self.state.selected_note_clip {
            if tid == track_id {
                let dup_btn = button(
                    row![
                        icons::icon(icons::COPY).size(10).color(th::TEXT_DIM),
                        text("Dup").size(10).color(th::TEXT_DIM)
                    ]
                    .spacing(2)
                    .align_y(iced::Alignment::Center),
                )
                .on_press(Message::DuplicateNoteClip(tid, cid))
                .padding([2, 6])
                .style(|_theme: &Theme, _status| button::Style {
                    background: Some(th::BG_ELEVATED.into()),
                    text_color: th::TEXT_DIM,
                    border: iced::Border {
                        color: th::BORDER,
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                });

                let double_btn = button(text("2x").size(10).color(th::TEXT_DIM))
                    .on_press(Message::DoubleNoteClip(tid, cid))
                    .padding([2, 6])
                    .style(|_theme: &Theme, _status| button::Style {
                        background: Some(th::BG_ELEVATED.into()),
                        text_color: th::TEXT_DIM,
                        border: iced::Border {
                            color: th::BORDER,
                            width: 1.0,
                            radius: 3.0.into(),
                        },
                        ..Default::default()
                    });

                let crop_btn = button(
                    row![
                        icons::icon(icons::SCISSORS).size(10).color(th::TEXT_DIM),
                        text("Crop").size(10).color(th::TEXT_DIM)
                    ]
                    .spacing(2)
                    .align_y(iced::Alignment::Center),
                )
                .on_press(Message::CropNoteClip(tid, cid))
                .padding([2, 6])
                .style(|_theme: &Theme, _status| button::Style {
                    background: Some(th::BG_ELEVATED.into()),
                    text_color: th::TEXT_DIM,
                    border: iced::Border {
                        color: th::BORDER,
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                });

                clip_ops = clip_ops.push(dup_btn).push(double_btn).push(crop_btn);
            }
        }

        // Snap grid selector
        use crate::state::SnapGrid;
        let mut snap_row = row![].spacing(2);
        for &grid in SnapGrid::all() {
            let is_active = self.state.snap_grid == grid;
            let (bg, text_color) = if is_active {
                (th::ACCENT_DIM, th::ACCENT)
            } else {
                (th::BG_ELEVATED, th::TEXT_DIM)
            };
            let btn = button(text(grid.label()).size(10).color(text_color))
                .on_press(Message::SetSnapGrid(grid))
                .padding([2, 6])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: Some(bg.into()),
                    text_color,
                    border: iced::Border {
                        color: if is_active {
                            th::ACCENT_DIM
                        } else {
                            th::BORDER
                        },
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                });
            snap_row = snap_row.push(btn);
        }
        let snap_label = text("Snap:").size(10).color(th::TEXT_DIM);
        let header_row = row![label, clip_ops, horizontal_space(), snap_label, snap_row]
            .spacing(4)
            .align_y(iced::Alignment::Center);

        let content = column![header_row, piano_canvas].spacing(4).padding(4);

        container(content)
            .width(Length::FillPortion(1))
            .height(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::BG_DARK.into()),
                border: iced::Border {
                    color: th::BORDER,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    /// Audio clip waveform panel for the detail panel split view.
    fn view_audio_clip_panel(&self, clip: &UiClip, track_color: Color) -> Element<'_, Message> {
        let playhead_samples = self.state.position_samples;
        let playhead_normalized = if clip.duration > 0
            && playhead_samples >= clip.position
            && playhead_samples < clip.position + clip.duration
        {
            (playhead_samples - clip.position) as f64 / clip.duration as f64
        } else {
            -1.0
        };

        let waveform_widget = AudioClipDetailWidget {
            audio: Arc::clone(&clip.audio),
            duration_samples: clip.duration,
            source_offset: clip.source_offset,
            sample_rate: self.state.sample_rate,
            track_color,
            playhead_normalized,
        };

        let waveform_canvas: Element<'_, Message> = canvas(waveform_widget)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();

        let label = text("Waveform").size(11).color(th::TEXT_DIM);
        let clip_info = text(format!(
            "{} — {:.1}s",
            clip.name,
            clip.duration as f64 / self.state.sample_rate as f64
        ))
        .size(10)
        .color(th::TEXT_MUTED);

        let header_row = row![label, horizontal_space(), clip_info]
            .spacing(4)
            .align_y(iced::Alignment::Center);

        let content = column![header_row, waveform_canvas].spacing(4).padding(4);

        container(content)
            .width(Length::FillPortion(1))
            .height(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::BG_DARK.into()),
                border: iced::Border {
                    color: th::BORDER,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    // ── Transport bar ──

    fn view_transport(&self) -> Element<'_, Message> {
        // Skip back button
        let skip_back_btn = button(icons::icon(icons::SKIP_BACK).size(16).color(th::TEXT))
            .on_press(Message::Stop)
            .padding([8, 12])
            .style(|_theme: &Theme, _status| button::Style {
                background: Some(th::BG_ELEVATED.into()),
                text_color: th::TEXT,
                border: iced::Border {
                    color: th::BORDER,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            });

        // Play/Pause button
        let play_pause_btn = if self.state.playing {
            button(icons::icon(icons::PAUSE).size(16).color(th::ACCENT))
                .on_press(Message::Stop)
                .padding([8, 14])
                .style(|_theme: &Theme, _status| button::Style {
                    background: Some(th::BG_ELEVATED.into()),
                    text_color: th::ACCENT,
                    border: iced::Border {
                        color: th::ACCENT_DIM,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                })
        } else {
            button(icons::icon(icons::PLAY).size(16).color(th::SUCCESS))
                .on_press(Message::Play)
                .padding([8, 14])
                .style(|_theme: &Theme, _status| button::Style {
                    background: Some(th::BG_ELEVATED.into()),
                    text_color: th::SUCCESS,
                    border: iced::Border {
                        color: th::BORDER,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                })
        };

        let transport_buttons = row![skip_back_btn, play_pause_btn].spacing(4);

        // Time display
        let time_text = text(format!(
            "{} / {}",
            AppState::format_time(self.state.position_seconds()),
            AppState::format_time(self.state.duration_seconds()),
        ))
        .size(14)
        .color(th::TEXT);

        // BPM
        let bpm_input = text_input("BPM", &self.state.bpm_text)
            .on_input(Message::BpmChanged)
            .on_submit(Message::BpmSubmit)
            .width(Length::Fixed(55.0))
            .size(14);

        let bpm_label = text("BPM").size(12).color(th::TEXT_DIM);

        // Master VU meter
        let master_meter = VuMeterWidget {
            peak_l: self.state.peak_l,
            peak_r: self.state.peak_r,
        };
        let master_meter_canvas: Element<'_, Message> = canvas(master_meter)
            .width(Length::Fixed(24.0))
            .height(Length::Fixed(28.0))
            .into();

        let volume_icon = icons::icon(icons::VOLUME_2).size(14).color(th::TEXT_DIM);

        let transport = row![
            transport_buttons,
            horizontal_space(),
            time_text,
            horizontal_space(),
            volume_icon,
            master_meter_canvas,
            bpm_input,
            bpm_label,
        ]
        .spacing(12)
        .padding(10)
        .align_y(iced::Alignment::Center);

        container(transport)
            .width(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::BG_SURFACE.into()),
                border: iced::Border {
                    color: th::BORDER,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    fn view_status(&self) -> Element<'_, Message> {
        let status = text(&self.state.status_text).size(11).color(th::TEXT_DIM);

        container(status)
            .width(Length::Fill)
            .padding([3, 12])
            .style(|_theme: &Theme| container::Style {
                background: Some(th::BG_DARK.into()),
                ..Default::default()
            })
            .into()
    }

    fn theme(&self) -> Theme {
        th::vibez_theme()
    }

    fn subscription(&self) -> Subscription<Message> {
        iced::time::every(std::time::Duration::from_millis(UI_TICK_MS)).map(|_| Message::Tick)
    }
}

async fn decode_file_async(
    path: PathBuf,
) -> Result<vibez_core::audio_buffer::DecodedAudio, String> {
    tokio::task::spawn_blocking(move || {
        file_io::decode_audio_file(&path).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("decode task failed: {e}"))?
}
