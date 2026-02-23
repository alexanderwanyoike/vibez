use std::path::PathBuf;
use std::sync::Arc;

use iced::widget::{
    button, canvas, center, column, container, horizontal_space, row, scrollable, text, text_input,
};
use iced::{Element, Length, Subscription, Task, Theme};

use rtrb::{Consumer, Producer};
use vibez_audio_io::audio_stream::AudioOutputStream;
use vibez_audio_io::file_io;
use vibez_core::constants::UI_TICK_MS;
use vibez_core::effect::EffectType;
use vibez_core::id::{ClipId, EffectId, TrackId};
use vibez_core::midi::{InstrumentKind, MidiNote, TrackKind};
use vibez_engine::commands::EngineCommand;
use vibez_engine::engine::AudioEngine;
use vibez_engine::events::EngineEvent;

use crate::message::Message;
use crate::state::{AppState, UiClip, UiEffect, UiNoteClip, UiTrack, Workspace};
use crate::theme as vibez_theme;
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
                self.state.next_track_number += 1;
                let id = TrackId::new();
                let name = format!("Track {track_num}");

                self.send_command(EngineCommand::AddTrack(id, name.clone()));
                self.state.tracks.push(UiTrack::new(id, name));
                self.state.selected_track = Some(id);
                self.state.status_text = format!("{} tracks", self.state.tracks.len());
            }
            Message::RemoveTrack(track_id) => {
                self.send_command(EngineCommand::RemoveTrack(track_id));
                self.state.tracks.retain(|t| t.id != track_id);
                if self.state.selected_track == Some(track_id) {
                    self.state.selected_track = self.state.tracks.first().map(|t| t.id);
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
                });

                if let Some(track) = self.state.find_track_mut(track_id) {
                    track.clips.push(UiClip {
                        id: clip_id,
                        name: name.clone(),
                        audio,
                        position: existing_end,
                        source_offset: 0,
                        duration,
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
                let effect_id = EffectId::new();
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
                self.state.status_text = format!("Added {} effect", effect_type.name());
            }
            Message::RemoveEffect(track_id, effect_id) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    track.effects.retain(|e| e.id != effect_id);
                }
                self.state.status_text = "Removed effect".to_string();
            }
            Message::SetEffectParam(track_id, effect_id, param_index, value) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(effect) = track.effects.iter_mut().find(|e| e.id == effect_id) {
                        if param_index < effect.params.len() {
                            let desc = &effect.descriptors[param_index];
                            effect.params[param_index] = value.clamp(desc.min, desc.max);
                        }
                    }
                }
            }
            Message::ToggleEffectBypass(track_id, effect_id) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(effect) = track.effects.iter_mut().find(|e| e.id == effect_id) {
                        effect.bypass = !effect.bypass;
                    }
                }
            }
            Message::MoveEffectUp(track_id, effect_id) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(idx) = track.effects.iter().position(|e| e.id == effect_id) {
                        if idx > 0 {
                            track.effects.swap(idx, idx - 1);
                        }
                    }
                }
            }
            Message::MoveEffectDown(track_id, effect_id) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(idx) = track.effects.iter().position(|e| e.id == effect_id) {
                        if idx + 1 < track.effects.len() {
                            track.effects.swap(idx, idx + 1);
                        }
                    }
                }
            }

            // -- Instrument tracks --
            Message::AddInstrumentTrack => {
                let track_num = self.state.next_track_number;
                self.state.next_track_number += 1;
                let id = TrackId::new();
                let name = format!("Synth {track_num}");
                let kind = TrackKind::Instrument(InstrumentKind::SubtractiveSynth);

                self.send_command(EngineCommand::AddTrack(id, name.clone()));
                self.state
                    .tracks
                    .push(UiTrack::new_instrument(id, name, kind));
                self.state.selected_track = Some(id);
                self.state.status_text = format!("{} tracks", self.state.tracks.len());
            }
            Message::SetSynthParam(track_id, param_index, value) => {
                // Synth params would be stored per-track; for now, update status
                let _ = (track_id, param_index, value);
                self.state.status_text = format!("Synth param {param_index} = {value:.2}");
            }

            // -- Piano roll --
            Message::AddNoteClipToTrack(track_id) => {
                let clip_id = ClipId::new();
                if let Some(track) = self.state.find_track_mut(track_id) {
                    track.note_clips.push(UiNoteClip {
                        id: clip_id,
                        name: format!("Pattern {}", track.note_clips.len() + 1),
                        position_beats: 0.0,
                        duration_beats: 16.0,
                        notes: Vec::new(),
                        selected_note: None,
                    });
                }
                self.state.status_text = "Added note clip".to_string();
            }
            Message::AddNote {
                track_id,
                clip_id,
                pitch,
                start_beat,
                duration_beats,
            } => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.notes.push(MidiNote {
                            pitch,
                            velocity: 100,
                            start_beat,
                            duration_beats,
                        });
                    }
                }
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
            }
            Message::EditNote(track_id, clip_id, note_index, new_note) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        if note_index < clip.notes.len() {
                            clip.notes[note_index] = new_note;
                        }
                    }
                }
            }
            Message::SelectNote(track_id, clip_id, note_index) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.selected_note = note_index;
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
            Workspace::Piano => self.view_piano(),
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
        let title = text("vibez").size(24).color(vibez_theme::ACCENT);

        // Workspace tabs
        let arrange_tab = {
            let color = if self.state.workspace == Workspace::Arrange {
                vibez_theme::ACCENT
            } else {
                vibez_theme::TEXT_DIM
            };
            button(text("Arrange").size(13).color(color))
                .on_press(Message::SwitchWorkspace(Workspace::Arrange))
                .padding([6, 14])
        };

        let mix_tab = {
            let color = if self.state.workspace == Workspace::Mix {
                vibez_theme::ACCENT
            } else {
                vibez_theme::TEXT_DIM
            };
            button(text("Mix").size(13).color(color))
                .on_press(Message::SwitchWorkspace(Workspace::Mix))
                .padding([6, 14])
        };

        let piano_tab = {
            let color = if self.state.workspace == Workspace::Piano {
                vibez_theme::ACCENT
            } else {
                vibez_theme::TEXT_DIM
            };
            button(text("Piano").size(13).color(color))
                .on_press(Message::SwitchWorkspace(Workspace::Piano))
                .padding([6, 14])
        };

        let tabs = row![arrange_tab, mix_tab, piano_tab].spacing(2);

        let add_audio_btn = button(text("+ Audio").size(14))
            .on_press(Message::AddTrack)
            .padding([6, 16]);

        let add_synth_btn = button(text("+ Synth").size(14))
            .on_press(Message::AddInstrumentTrack)
            .padding([6, 16]);

        let mut header_row = row![
            title,
            tabs,
            horizontal_space(),
            add_audio_btn,
            add_synth_btn
        ]
        .spacing(8);

        if let Some(selected_id) = self.state.selected_track {
            let remove_btn = button(text("Remove Track").size(14))
                .on_press(Message::RemoveTrack(selected_id))
                .padding([6, 16]);
            header_row = header_row.push(remove_btn);
        }

        let header = header_row.padding(12).align_y(iced::Alignment::Center);

        container(header)
            .width(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(vibez_theme::BG_SURFACE.into()),
                ..Default::default()
            })
            .into()
    }

    // ── Arrangement view ──

    fn view_arrangement(&self) -> Element<'_, Message> {
        if self.state.tracks.is_empty() {
            let prompt = text("Add a track to get started")
                .size(16)
                .color(vibez_theme::TEXT_DIM);

            let centered = center(prompt).width(Length::Fill).height(Length::Fill);

            return container(centered)
                .width(Length::Fill)
                .height(Length::FillPortion(5))
                .style(|_theme: &Theme| container::Style {
                    background: Some(vibez_theme::BG_DARK.into()),
                    ..Default::default()
                })
                .into();
        }

        let playhead = self.state.position_normalized();
        let duration = self.state.duration_seconds();
        let sample_rate = self.state.sample_rate;

        // Time ruler across the top (offset by track header width)
        let ruler = RulerWidget {
            playhead_position: playhead,
            duration_seconds: duration,
        };
        let ruler_canvas: Element<Message> = canvas(ruler)
            .width(Length::Fill)
            .height(Length::Fixed(24.0))
            .into();

        // Spacer matching header width for the ruler row
        let ruler_spacer = container(text(""))
            .width(Length::Fixed(
                crate::widgets::track_header::TRACK_HEADER_WIDTH,
            ))
            .height(Length::Fixed(24.0));

        let ruler_row = row![ruler_spacer, ruler_canvas];

        // Track rows: header widgets + clip canvas
        let mut track_rows = column![].spacing(0);

        for track in &self.state.tracks {
            let selected = self.state.selected_track == Some(track.id);

            // Track header (iced widgets)
            let header = view_track_header(track);

            // Clip canvas for this track
            let clip_canvas_widget =
                TrackClipCanvas::from_track(track, playhead, duration, sample_rate, selected);
            let clip_canvas: Element<Message> = canvas(clip_canvas_widget)
                .width(Length::Fill)
                .height(Length::Fixed(80.0))
                .into();

            let track_row = row![header, clip_canvas].height(Length::Fixed(80.0));

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
                background: Some(vibez_theme::BG_DARK.into()),
                ..Default::default()
            })
            .into()
    }

    // ── Mixer view ──

    fn view_mixer(&self) -> Element<'_, Message> {
        if self.state.tracks.is_empty() {
            let prompt = text("Add a track to get started")
                .size(16)
                .color(vibez_theme::TEXT_DIM);

            let centered = center(prompt).width(Length::Fill).height(Length::Fill);

            return container(centered)
                .width(Length::Fill)
                .height(Length::FillPortion(5))
                .style(|_theme: &Theme| container::Style {
                    background: Some(vibez_theme::BG_DARK.into()),
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

        // Master strip — pinned to far right, fills height
        let master_label = text("Master").size(11).color(vibez_theme::TEXT);
        let master_meter = VuMeterWidget {
            peak_l: self.state.peak_l,
            peak_r: self.state.peak_r,
        };
        let master_meter_canvas: Element<Message> = canvas(master_meter)
            .width(Length::Fixed(28.0))
            .height(Length::Fill)
            .into();

        let master_col = column![master_label, master_meter_canvas]
            .spacing(4)
            .padding(6)
            .width(Length::Fixed(60.0))
            .height(Length::Fill)
            .align_x(iced::Alignment::Center);

        let master_container =
            container(master_col)
                .height(Length::Fill)
                .style(|_theme: &Theme| container::Style {
                    background: Some(vibez_theme::BG_SURFACE.into()),
                    border: iced::Border {
                        color: vibez_theme::ACCENT,
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
                background: Some(vibez_theme::BG_SURFACE.into()),
                ..Default::default()
            })
            .into()
    }

    // ── Piano roll view ──

    fn view_piano(&self) -> Element<'_, Message> {
        let selected = self
            .state
            .selected_track
            .and_then(|id| self.state.find_track(id));

        let playhead_beats = self.state.position_beats();

        let piano_widget = if let Some(track) = selected {
            if let Some(clip) = track.note_clips.first() {
                PianoRollWidget::from_clip(track.id, clip, playhead_beats, clip.duration_beats)
            } else {
                PianoRollWidget::empty(track.id, playhead_beats)
            }
        } else {
            PianoRollWidget::empty(TrackId::new(), playhead_beats)
        };

        let piano_canvas: Element<Message> = canvas(piano_widget)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();

        // Header row with add-pattern button if an instrument track is selected
        let mut piano_header = row![].spacing(8).padding([4, 8]);

        if let Some(track) = selected {
            let track_label = text(format!("Piano Roll: {}", track.name))
                .size(13)
                .color(vibez_theme::TEXT);
            piano_header = piano_header.push(track_label);

            if matches!(track.kind, TrackKind::Instrument(_)) {
                let add_pattern = button(text("+ Pattern").size(12))
                    .on_press(Message::AddNoteClipToTrack(track.id))
                    .padding([4, 10]);
                piano_header = piano_header.push(add_pattern);
            }
        } else {
            let label = text("Select an instrument track")
                .size(13)
                .color(vibez_theme::TEXT_DIM);
            piano_header = piano_header.push(label);
        }

        let content = column![piano_header, piano_canvas];

        container(content)
            .width(Length::Fill)
            .height(Length::FillPortion(5))
            .style(|_theme: &Theme| container::Style {
                background: Some(vibez_theme::BG_DARK.into()),
                ..Default::default()
            })
            .into()
    }

    // ── Detail panel (global — effects/instruments for selected track) ──

    fn view_detail_panel(&self) -> Element<'_, Message> {
        let detail_content: Element<Message> = if let Some(track) = self
            .state
            .selected_track
            .and_then(|id| self.state.find_track(id))
        {
            let track_id = track.id;

            // Effect chain section
            let mut effects_row = row![].spacing(6);

            for effect in &track.effects {
                let slot = view_effect_slot(track_id, effect);
                effects_row = effects_row.push(slot);
            }

            // Add effect buttons
            let mut add_effects_col =
                column![text("Add Effect").size(10).color(vibez_theme::TEXT_DIM),].spacing(2);

            for &et in EffectType::all() {
                let btn = button(text(et.name()).size(10))
                    .on_press(Message::AddEffect(track_id, et))
                    .padding([3, 8]);
                add_effects_col = add_effects_col.push(btn);
            }

            effects_row = effects_row.push(add_effects_col);

            // Synth params section for instrument tracks
            let synth_section: Element<Message> = if matches!(track.kind, TrackKind::Instrument(_))
            {
                let label = text("Synth Parameters")
                    .size(11)
                    .color(vibez_theme::TEXT_DIM);
                let placeholder = text("(synth UI coming in Phase 4)")
                    .size(10)
                    .color(vibez_theme::TEXT_DIM);
                column![label, placeholder]
                    .spacing(4)
                    .padding([0, 8])
                    .into()
            } else {
                text("").into()
            };

            let label = text(format!("{} — Effects", track.name))
                .size(14)
                .color(vibez_theme::TEXT);

            let scrollable_effects = scrollable(effects_row).direction(
                scrollable::Direction::Horizontal(scrollable::Scrollbar::default()),
            );

            column![label, scrollable_effects, synth_section]
                .spacing(6)
                .padding(8)
                .into()
        } else {
            let label = text("Select a track to view effects / instruments")
                .size(14)
                .color(vibez_theme::TEXT_DIM);
            center(label)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        };

        container(detail_content)
            .width(Length::Fill)
            .height(Length::FillPortion(2))
            .style(|_theme: &Theme| container::Style {
                background: Some(vibez_theme::BG_DARK.into()),
                border: iced::Border {
                    color: iced::Color {
                        a: 0.2,
                        ..vibez_theme::TEXT_DIM
                    },
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    // ── Transport bar ──

    fn view_transport(&self) -> Element<'_, Message> {
        let play_btn = if self.state.playing {
            button(text("Stop").size(14))
                .on_press(Message::Stop)
                .padding([8, 20])
        } else {
            button(text("Play").size(14))
                .on_press(Message::Play)
                .padding([8, 20])
        };

        let time_text = text(format!(
            "{} / {}",
            AppState::format_time(self.state.position_seconds()),
            AppState::format_time(self.state.duration_seconds()),
        ))
        .size(16)
        .color(vibez_theme::TEXT);

        let bpm_input = text_input("BPM", &self.state.bpm_text)
            .on_input(Message::BpmChanged)
            .on_submit(Message::BpmSubmit)
            .width(Length::Fixed(60.0))
            .size(14);

        let bpm_label = text("BPM").size(12).color(vibez_theme::TEXT_DIM);

        let track_count = if !self.state.tracks.is_empty() {
            text(format!("{} tracks", self.state.tracks.len()))
                .size(12)
                .color(vibez_theme::TEXT_DIM)
        } else {
            text("").size(12).color(vibez_theme::TEXT_DIM)
        };

        // Master VU meter in the transport bar
        let master_meter = VuMeterWidget {
            peak_l: self.state.peak_l,
            peak_r: self.state.peak_r,
        };
        let master_meter_canvas: Element<Message> = canvas(master_meter)
            .width(Length::Fixed(24.0))
            .height(Length::Fixed(28.0))
            .into();

        let transport = row![
            play_btn,
            horizontal_space(),
            time_text,
            horizontal_space(),
            master_meter_canvas,
            track_count,
            bpm_input,
            bpm_label,
        ]
        .spacing(12)
        .padding(12)
        .align_y(iced::Alignment::Center);

        container(transport)
            .width(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(vibez_theme::BG_SURFACE.into()),
                ..Default::default()
            })
            .into()
    }

    fn view_status(&self) -> Element<'_, Message> {
        let status = text(&self.state.status_text)
            .size(12)
            .color(vibez_theme::TEXT_DIM);

        container(status)
            .width(Length::Fill)
            .padding([4, 12])
            .into()
    }

    fn theme(&self) -> Theme {
        vibez_theme::vibez_theme()
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
