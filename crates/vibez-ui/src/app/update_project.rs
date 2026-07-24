//! Routes Project state and project-file messages.

use crate::domains::project::{ProjectCtx, ProjectMsg};

use super::*;

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
        self.state.project.file_menu_open = false;
        self.reset_to_new_project();
        Task::none()
    }

    pub(super) fn route_open_project(&mut self) -> Task<Message> {
        self.state.project.file_menu_open = false;
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
        self.state.project.file_menu_open = false;
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
        self.state.project.file_menu_open = false;
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
            }
            Err(err) => {
                self.state.status_text = format!("Project save error: {err}");
            }
        }
        Task::none()
    }
}
