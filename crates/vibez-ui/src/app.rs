use std::path::PathBuf;
use std::sync::Arc;

use iced::widget::{button, canvas, column, container, horizontal_space, row, text, text_input};
use iced::{Element, Length, Subscription, Task, Theme};

use rtrb::{Consumer, Producer};
use vibez_audio_io::audio_stream::AudioOutputStream;
use vibez_audio_io::file_io;
use vibez_core::constants::UI_TICK_MS;
use vibez_engine::commands::EngineCommand;
use vibez_engine::engine::AudioEngine;
use vibez_engine::events::EngineEvent;

use crate::message::Message;
use crate::state::AppState;
use crate::theme as vibez_theme;
use crate::widgets::vu_meter::VuMeterWidget;
use crate::widgets::waveform::WaveformWidget;

struct App {
    state: AppState,
    waveform: WaveformWidget,
    vu_meter: VuMeterWidget,
    cmd_tx: Option<Producer<EngineCommand>>,
    event_rx: Option<Consumer<EngineEvent>>,
    _stream: Option<AudioOutputStream>,
}

pub fn run() -> iced::Result {
    iced::application("vibez", App::update, App::view)
        .theme(App::theme)
        .subscription(App::subscription)
        .window_size((1100.0, 700.0))
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
            }
            Message::TogglePlayback => {
                if self.state.playing {
                    return self.update(Message::Stop);
                } else {
                    return self.update(Message::Play);
                }
            }
            Message::Seek(normalized) => {
                if let Some(ref audio) = self.state.audio {
                    let sample_pos = (normalized * audio.num_frames() as f64) as u64;
                    self.state.position_samples = sample_pos;
                    self.send_command(EngineCommand::Seek(sample_pos));
                    self.waveform.set_playhead(normalized);
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
            }
            Message::EnginePosition(pos) => {
                self.state.position_samples = pos;
                self.waveform.set_playhead(self.state.position_normalized());
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
                        // Smooth the meter with a simple decay
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
                }
            }
        }
    }

    fn view(&self) -> Element<Message> {
        let header = self.view_header();
        let waveform_area = self.view_waveform();
        let transport_bar = self.view_transport();
        let status_bar = self.view_status();

        let layout = column![header, waveform_area, transport_bar, status_bar];

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

        let header = row![title, horizontal_space(), open_btn]
            .spacing(16)
            .padding(12)
            .align_y(iced::Alignment::Center);

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

        let transport = row![
            play_btn,
            horizontal_space(),
            time_text,
            horizontal_space(),
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
