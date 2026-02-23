use std::path::PathBuf;
use std::sync::Arc;

use iced::widget::{
    button, canvas, column, container, horizontal_space, row, scrollable, text, text_input,
};
use iced::{Element, Length, Subscription, Task, Theme};

use rtrb::{Consumer, Producer};
use vibez_audio_io::audio_stream::AudioOutputStream;
use vibez_audio_io::file_io;
use vibez_core::constants::UI_TICK_MS;
use vibez_core::id::{ClipId, TrackId};
use vibez_engine::commands::EngineCommand;
use vibez_engine::engine::AudioEngine;
use vibez_engine::events::EngineEvent;

use crate::message::Message;
use crate::state::{AppState, UiClip, UiTrack};
use crate::theme as vibez_theme;
use crate::widgets::mixer_strip::view_mixer_strip;
use crate::widgets::timeline::TimelineWidget;
use crate::widgets::vu_meter::VuMeterWidget;
use crate::widgets::waveform::WaveformWidget;

struct App {
    state: AppState,
    waveform: WaveformWidget,
    vu_meter: VuMeterWidget,
    timeline: TimelineWidget,
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

        (
            Self {
                state,
                waveform: WaveformWidget::new(),
                vu_meter: VuMeterWidget::new(),
                timeline: TimelineWidget::new(),
                cmd_tx: Some(cmd_tx),
                event_rx: Some(event_rx),
                _stream: stream,
            },
            Task::none(),
        )
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
                self.waveform.set_playhead(0.0);
                self.timeline.set_playhead(0.0);
            }
            Message::TogglePlayback => {
                if self.state.playing {
                    return self.update(Message::Stop);
                } else {
                    return self.update(Message::Play);
                }
            }
            Message::Seek(normalized) => {
                let duration_samples = self.state.total_duration_samples();
                let legacy_frames = self
                    .state
                    .audio
                    .as_ref()
                    .map_or(0, |a| a.num_frames() as u64);
                let total = if duration_samples > 0 {
                    duration_samples
                } else {
                    legacy_frames
                };

                if total > 0 {
                    let sample_pos = (normalized * total as f64) as u64;
                    self.state.position_samples = sample_pos;
                    self.send_command(EngineCommand::Seek(sample_pos));
                    self.waveform.set_playhead(normalized);
                    self.timeline.set_playhead(normalized);
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
            Message::OpenFile => {
                return Task::perform(
                    async {
                        let handle = rfd::AsyncFileDialog::new()
                            .set_title("Open Audio File")
                            .add_filter("Audio", &["wav", "mp3", "flac", "ogg"])
                            .pick_file()
                            .await;
                        handle.map(|h| h.path().to_path_buf())
                    },
                    Message::FileSelected,
                );
            }
            Message::FileSelected(path) => {
                if let Some(path) = path {
                    let file_name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    self.state.loading = true;
                    self.state.status_text = format!("Loading {file_name}...");
                    self.state.file_name = Some(file_name);

                    return Task::perform(decode_file_async(path), |result| match result {
                        Ok(audio) => Message::AudioDecoded(Arc::new(audio)),
                        Err(e) => Message::DecodeError(e),
                    });
                }
            }
            Message::AudioDecoded(audio) => {
                self.state.loading = false;
                self.state.sample_rate = audio.sample_rate;
                let name = self.state.file_name.as_deref().unwrap_or("audio");
                self.state.status_text = format!(
                    "{} — {:.1}s, {}ch, {}Hz",
                    name,
                    audio.duration_seconds(),
                    audio.num_channels(),
                    audio.sample_rate,
                );

                self.waveform.set_audio(Some(Arc::clone(&audio)));
                self.waveform.set_playhead(0.0);
                self.state.position_samples = 0;
                self.state.playing = false;

                self.send_command(EngineCommand::Stop);
                self.send_command(EngineCommand::Seek(0));
                self.send_command(EngineCommand::LoadAudio(audio));
                self.state.audio = self.waveform.audio.clone();
            }
            Message::DecodeError(err) => {
                self.state.loading = false;
                self.state.status_text = format!("Error: {err}");
            }
            Message::Tick => {
                self.poll_engine_events();
                // Sync timeline
                let selected_idx = self
                    .state
                    .selected_track
                    .and_then(|id| self.state.tracks.iter().position(|t| t.id == id));
                self.timeline.sync_from_tracks(
                    &self.state.tracks,
                    selected_idx,
                    self.state.duration_seconds(),
                    self.state.sample_rate,
                );
                self.timeline.set_playhead(self.state.position_normalized());
            }
            Message::EnginePosition(pos) => {
                self.state.position_samples = pos;
                self.waveform.set_playhead(self.state.position_normalized());
                self.timeline.set_playhead(self.state.position_normalized());
            }
            Message::EngineMetering { peak_l, peak_r } => {
                self.state.peak_l = peak_l;
                self.state.peak_r = peak_r;
                self.vu_meter.peak_l = peak_l;
                self.vu_meter.peak_r = peak_r;
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
                self.state.status_text = format!("{} tracks", self.state.tracks.len());
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
                // Place clip at the end of existing clips on this track
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

                // Send to engine
                self.send_command(EngineCommand::AddClip {
                    track_id,
                    clip_id,
                    audio: Arc::clone(&audio),
                    position: existing_end,
                    source_offset: 0,
                    duration,
                });

                // Update UI state
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
        }
        Task::none()
    }

    fn poll_engine_events(&mut self) {
        if let Some(ref mut rx) = self.event_rx {
            while let Ok(event) = rx.pop() {
                match event {
                    EngineEvent::PlaybackPosition(pos) => {
                        self.state.position_samples = pos;
                        self.waveform.set_playhead(self.state.position_normalized());
                    }
                    EngineEvent::Metering { peak_l, peak_r, .. } => {
                        self.state.peak_l = peak_l.max(self.state.peak_l * 0.85);
                        self.state.peak_r = peak_r.max(self.state.peak_r * 0.85);
                        self.vu_meter.peak_l = self.state.peak_l;
                        self.vu_meter.peak_r = self.state.peak_r;
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

    fn view(&self) -> Element<Message> {
        let header = self.view_header();

        let main_content = if self.state.tracks.is_empty() {
            // Legacy view: waveform + meter
            self.view_waveform()
        } else {
            // Multi-track view: timeline + mixer
            let timeline = self.view_timeline();
            let mixer = self.view_mixer();

            column![timeline, mixer].spacing(2).into()
        };

        let transport_bar = self.view_transport();
        let status_bar = self.view_status();

        let layout = column![header, main_content, transport_bar, status_bar];

        container(layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn view_header(&self) -> Element<Message> {
        let title = text("vibez").size(24).color(vibez_theme::ACCENT);

        let open_btn = button(text("Open").size(14))
            .on_press(Message::OpenFile)
            .padding([6, 16]);

        let add_track_btn = button(text("Add Track").size(14))
            .on_press(Message::AddTrack)
            .padding([6, 16]);

        let mut header_row = row![title, horizontal_space(), add_track_btn].spacing(8);

        // Show remove track button if a track is selected
        if let Some(selected_id) = self.state.selected_track {
            let remove_btn = button(text("Remove Track").size(14))
                .on_press(Message::RemoveTrack(selected_id))
                .padding([6, 16]);
            header_row = header_row.push(remove_btn);
        }

        header_row = header_row.push(open_btn);

        let header = header_row.padding(12).align_y(iced::Alignment::Center);

        container(header)
            .width(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(vibez_theme::BG_SURFACE.into()),
                ..Default::default()
            })
            .into()
    }

    fn view_waveform(&self) -> Element<Message> {
        let waveform_canvas: Element<Message> = canvas(&self.waveform)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();

        let meter_canvas: Element<Message> = canvas(&self.vu_meter)
            .width(Length::Fixed(40.0))
            .height(Length::Fill)
            .into();

        let content = row![waveform_canvas, meter_canvas].spacing(4);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(8)
            .into()
    }

    fn view_timeline(&self) -> Element<Message> {
        let timeline_canvas: Element<Message> = canvas(&self.timeline)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();

        container(timeline_canvas)
            .width(Length::Fill)
            .height(Length::FillPortion(6))
            .padding([0, 8])
            .into()
    }

    fn view_mixer(&self) -> Element<Message> {
        let mut strips = row![].spacing(4).padding(8);

        for track in &self.state.tracks {
            let strip = view_mixer_strip(track);
            strips = strips.push(strip);
        }

        // Master strip
        let master_label = text("Master").size(11).color(vibez_theme::TEXT);
        let master_meter = VuMeterWidget {
            peak_l: self.state.peak_l,
            peak_r: self.state.peak_r,
        };
        let master_meter_canvas: Element<Message> = canvas(master_meter)
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(100.0))
            .into();

        let master_strip = column![master_label, master_meter_canvas]
            .spacing(4)
            .padding(6)
            .width(Length::Fixed(60.0))
            .align_x(iced::Alignment::Center);

        let master_container = container(master_strip).style(|_theme: &Theme| container::Style {
            background: Some(vibez_theme::BG_SURFACE.into()),
            border: iced::Border {
                color: vibez_theme::ACCENT,
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        });

        strips = strips.push(master_container);

        let scrollable_mixer = scrollable(strips).direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::default(),
        ));

        container(scrollable_mixer)
            .width(Length::Fill)
            .height(Length::FillPortion(3))
            .style(|_theme: &Theme| container::Style {
                background: Some(vibez_theme::BG_DARK.into()),
                ..Default::default()
            })
            .into()
    }

    fn view_transport(&self) -> Element<Message> {
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

        let transport = row![
            play_btn,
            horizontal_space(),
            time_text,
            horizontal_space(),
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

    fn view_status(&self) -> Element<Message> {
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
    // Run decoding on a blocking thread
    tokio::task::spawn_blocking(move || {
        file_io::decode_audio_file(&path).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("decode task failed: {e}"))?
}
