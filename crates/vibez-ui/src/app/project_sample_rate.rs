//! Reloading the open project when the device's sample rate changes.
//!
//! Audio clips count their frames at the project's sample rate and their
//! audio must match the engine's, so a switch reloads the project through
//! the same conversion opening one uses.

use iced::Task;
use vibez_project::Project;

use super::tasks::{convert_project_sample_rate_async, SampleRateConversion};
use super::App;
use crate::message::{ConvertedProject, Message};

impl App {
    pub(super) fn sample_rate_conversion(
        &self,
        at_previous_rate: Project,
    ) -> Option<SampleRateConversion> {
        let sample_rate = self.state.transport.sample_rate;
        (at_previous_rate.sample_rate != sample_rate && at_previous_rate.holds_audio()).then(|| {
            SampleRateConversion {
                project: at_previous_rate,
                path: self.state.project.current_path.clone(),
                dirty: self.state.project.dirty,
                sample_rate,
            }
        })
    }

    pub(super) fn convert_project_sample_rate(
        &mut self,
        at_previous_rate: Project,
    ) -> Task<Message> {
        let Some(conversion) = self.sample_rate_conversion(at_previous_rate) else {
            return Task::none();
        };
        self.state.status_text = format!("Converting project to {} Hz...", conversion.sample_rate);
        let dropbox = self
            .dropbox_client
            .clone()
            .map(|client| (client, self.dropbox_cache.clone()));
        Task::perform(
            convert_project_sample_rate_async(conversion, dropbox),
            |converted| Message::ProjectSampleRateConverted(Box::new(converted)),
        )
    }

    pub(super) fn finish_sample_rate_conversion(&mut self, converted: ConvertedProject) {
        let sample_rate = converted.loaded.project.sample_rate;
        // A later switch has its own conversion on the way.
        if sample_rate != self.state.transport.sample_rate {
            return;
        }
        self.rebuild_from_loaded_project(converted.loaded);
        self.state.project.current_path = converted.path;
        self.state.project.dirty = false;
        if converted.dirty {
            self.mark_project_dirty();
        }
        self.state.status_text = format!("Converted project to {sample_rate} Hz");
    }
}
