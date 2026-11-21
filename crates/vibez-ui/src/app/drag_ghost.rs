//! Reads the file a drag carries, once and off the UI thread, for the ghost
//! that shows where its drop lands.

use iced::Task;
use vibez_core::track::MediaSourceRef;

use super::midi_file_drop::read_midi_file_async;
use super::tasks::decode_local_for_preview_async;
use super::App;
use crate::domains::browser::BrowserMsg;
use crate::domains::drop_ghost::{audio_clip, DragGhostRead, GhostMedia};
use crate::message::{AnalysedBrowserAudio, Message};

impl App {
    /// Reads `source` for its ghost; audio the Browser already decoded for
    /// its waveform is not decoded again.
    pub(super) fn read_drag_ghost(&self, source: MediaSourceRef) -> Task<Message> {
        let finished = ghost_read(source.clone());
        if let MediaSourceRef::LocalFile { path } = &source {
            if vibez_core::midi_file::is_midi_file_path(path) {
                return Task::perform(read_midi_file_async(path.clone()), move |import| {
                    finished(import.map(|import| GhostMedia::midi(&import)))
                });
            }
        }
        let name = source.display_name();
        if let Some(audio) = self.state.browser.waveform_for_source(&source) {
            let analysed = AnalysedBrowserAudio {
                audio,
                loop_fit: self.state.browser.waveform_loop_fit,
            };
            return Task::perform(audio_ghost(name, source, Ok(analysed)), finished);
        }
        let MediaSourceRef::LocalFile { path } = source.clone() else {
            return Task::none();
        };
        Task::perform(
            async move { audio_ghost(name, source, decode_local_for_preview_async(path).await).await },
            finished,
        )
    }
}

/// The message that hands what was read of `source` to its drag.
fn ghost_read(source: MediaSourceRef) -> impl Fn(Result<GhostMedia, String>) -> Message {
    move |media| {
        Message::Browser(BrowserMsg::DragGhostRead(media.map(|media| {
            Box::new(DragGhostRead {
                source: source.clone(),
                media,
            })
        })))
    }
}

/// The ghost of decoded audio, its waveform worked out off the UI thread.
async fn audio_ghost(
    name: String,
    source: MediaSourceRef,
    analysed: Result<AnalysedBrowserAudio, String>,
) -> Result<GhostMedia, String> {
    let AnalysedBrowserAudio { audio, loop_fit } = analysed?;
    tokio::task::spawn_blocking(move || {
        let clip = audio_clip(name, source, audio);
        let peaks = crate::widgets::timeline::compute_clip_peaks(&clip);
        GhostMedia::Audio {
            clip: Box::new(clip),
            loop_fit,
            peaks,
        }
    })
    .await
    .map_err(|error| error.to_string())
}
