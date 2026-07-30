//! Routes Project state and project-file messages.

use crate::domains::project::{ProjectCtx, ProjectMsg};

use super::window_policy::{close_request_decision, CloseRequest};
use super::*;

/// Quits the application. iced 0.13 has no "quit" task; the run loop ends when
/// the last window closes, and the window has to be looked up because the
/// close request carries no id through our message type.
fn exit_application() -> Task<Message> {
    iced::window::get_latest().and_then(iced::window::close)
}

impl App {
    pub(super) fn route_project_message(&mut self, msg: ProjectMsg) -> Task<Message> {
        if matches!(&msg, ProjectMsg::ToggleFileMenu) {
            self.state.view.edit_menu_open = false;
        }
        let ctx = ProjectCtx {
            snapshot_now: self.take_snapshot(),
        };
        let action = self.state.project.update(msg, ctx);
        if let Some(status) = action.status {
            self.state.status_text = status;
        }
        if let Some(snapshot) = action.apply_snapshot {
            self.apply_snapshot(snapshot);
        }
        Task::none()
    }

    pub(super) fn route_new_project(&mut self) -> Task<Message> {
        self.reset_to_new_project();
        Task::none()
    }

    pub(super) fn route_open_project(&mut self) -> Task<Message> {
        Task::perform(
            async {
                let handle = rfd::AsyncFileDialog::new()
                    .set_title("Open Vibez Project")
                    .add_filter("Vibez Project", &["vzp", "vibez", "json"])
                    .pick_file()
                    .await;
                handle.map(|file| file.path().to_path_buf())
            },
            Message::ProjectOpenPathSelected,
        )
    }

    pub(super) fn route_save_project(&mut self) -> Task<Message> {
        let project = self.project_for_save();
        if let Some(path) = self.state.project.current_path.clone() {
            return Task::perform(
                save_project_async(path.clone(), Some(path), project),
                |result| Message::ProjectSaved(Box::new(result)),
            );
        }
        self.update(Message::SaveProjectAs)
    }

    pub(super) fn route_save_project_as(&mut self) -> Task<Message> {
        Task::perform(
            async {
                let handle = rfd::AsyncFileDialog::new()
                    .set_title("Save Vibez Project")
                    .set_file_name("Untitled.vzp")
                    .add_filter("Vibez Project Format V1", &["vzp"])
                    .save_file()
                    .await;
                handle.map(|file| file.path().to_path_buf())
            },
            Message::ProjectSavePathSelected,
        )
    }

    pub(super) fn route_project_open_path_selected(
        &mut self,
        path: Option<PathBuf>,
    ) -> Task<Message> {
        if let Some(path) = path {
            self.state.status_text = format!("Opening {}...", path.display());
            let dropbox = self
                .dropbox_client
                .clone()
                .map(|client| (client, self.dropbox_cache.clone()));
            return Task::perform(load_project_async(path, dropbox), |result| {
                Message::ProjectLoaded(Box::new(result))
            });
        }
        Task::none()
    }

    pub(super) fn route_project_save_path_selected(
        &mut self,
        path: Option<PathBuf>,
    ) -> Task<Message> {
        if let Some(mut path) = path {
            if !path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("vzp"))
            {
                path.set_extension("vzp");
            }
            let project = self.project_for_save();
            return Task::perform(
                save_project_async(path, self.state.project.current_path.clone(), project),
                |result| Message::ProjectSaved(Box::new(result)),
            );
        }
        // Backing out of save-as also backs out of the quit that asked for it.
        self.state.project.exit_after_save = false;
        Task::none()
    }

    pub(super) fn route_project_loaded(
        &mut self,
        result: Result<ProjectLoadResult, String>,
    ) -> Task<Message> {
        match result {
            Ok(loaded) => {
                self.rebuild_from_loaded_project(loaded);
            }
            Err(err) => {
                self.state.status_text = format!("Project load error: {err}");
            }
        }
        Task::none()
    }

    pub(super) fn route_project_saved(
        &mut self,
        result: Result<ProjectSaveResult, String>,
    ) -> Task<Message> {
        match result {
            Ok(saved) => {
                self.apply_saved_project_sources(&saved.project);
                self.state.project.current_path = Some(saved.path.clone());
                self.state.project.dirty = false;
                self.state.status_text = format!("Saved {}", saved.path.display());
                if std::mem::take(&mut self.state.project.exit_after_save) {
                    return exit_application();
                }
            }
            Err(err) => {
                self.state.status_text = format!("Project save error: {err}");
                // A failed write must not take the project down with it; the
                // error stays on screen with the edits intact.
                self.state.project.exit_after_save = false;
            }
        }
        Task::none()
    }

    pub(super) fn route_window_close_requested(&mut self) -> Task<Message> {
        match close_request_decision(self.state.project.dirty) {
            CloseRequest::Exit => exit_application(),
            CloseRequest::Confirm => {
                self.state.project.close_confirm_open = true;
                Task::none()
            }
        }
    }

    pub(super) fn route_close_confirm_save(&mut self) -> Task<Message> {
        self.state.project.close_confirm_open = false;
        self.state.project.exit_after_save = true;
        self.update(Message::SaveProject)
    }

    pub(super) fn route_close_confirm_discard(&mut self) -> Task<Message> {
        self.state.project.close_confirm_open = false;
        exit_application()
    }

    pub(super) fn route_close_confirm_cancel(&mut self) -> Task<Message> {
        self.state.project.close_confirm_open = false;
        self.state.project.exit_after_save = false;
        Task::none()
    }
}
