//! Window title derivation and the close-request decision.
//!
//! Both rules are free functions over plain values rather than methods on
//! [`super::App`], because `App` owns an audio engine and cannot be built in a
//! test. Keeping the rules here is what makes the title format and the
//! "does quitting need a prompt" policy directly assertable.

use std::path::Path;

/// Name used for a project that has never been written to disk. Shared with
/// `project_from_state` so the title bar and the serialized project agree on
/// what an unsaved project is called.
pub(super) const UNTITLED_PROJECT_NAME: &str = "Untitled";

/// Marker appended to the title while the project holds unsaved edits. A
/// trailing glyph rather than a prefix so the project name stays the part of
/// the title that window lists and taskbars truncate last.
const DIRTY_MARKER: &str = " *";

/// The project's display name: the file stem of its path, or
/// [`UNTITLED_PROJECT_NAME`] before the first save. Deliberately the stem and
/// not the full path, so the title stays readable for deeply nested files.
pub(super) fn project_display_name(current_path: Option<&Path>) -> String {
    current_path
        .and_then(|path| path.file_stem())
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| UNTITLED_PROJECT_NAME.to_string())
}

/// The full window title for the given project path and dirty flag.
pub(super) fn window_title(current_path: Option<&Path>, dirty: bool) -> String {
    let name = project_display_name(current_path);
    let marker = if dirty { DIRTY_MARKER } else { "" };
    format!("vibez - {name}{marker}")
}

/// What a window close request should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CloseRequest {
    /// Nothing would be lost; let the window go.
    Exit,
    /// Unsaved edits exist; put the save/discard/cancel dialog up instead.
    Confirm,
}

/// Decides a close request purely from the dirty flag. A repeated request
/// while the dialog is already up lands here again and re-affirms `Confirm`,
/// so hammering the window button can never race past the prompt.
pub(super) fn close_request_decision(dirty: bool) -> CloseRequest {
    if dirty {
        CloseRequest::Confirm
    } else {
        CloseRequest::Exit
    }
}

#[cfg(test)]
mod tests {
    use super::{close_request_decision, project_display_name, window_title, CloseRequest};
    use std::path::Path;

    #[test]
    fn a_saved_project_titles_with_its_file_stem_not_its_path() {
        assert_eq!(
            window_title(Some(Path::new("/home/alex/music/Night Drive.vzp")), false),
            "vibez - Night Drive"
        );
        assert_eq!(
            project_display_name(Some(Path::new("/home/alex/music/Night Drive.vzp"))),
            "Night Drive"
        );
    }

    #[test]
    fn a_never_saved_project_titles_as_untitled() {
        assert_eq!(window_title(None, false), "vibez - Untitled");
    }

    #[test]
    fn unsaved_changes_append_the_dirty_marker_for_named_and_untitled_projects() {
        assert_eq!(
            window_title(Some(Path::new("/tmp/Night Drive.vzp")), true),
            "vibez - Night Drive *"
        );
        assert_eq!(window_title(None, true), "vibez - Untitled *");
    }

    #[test]
    fn the_dirty_marker_disappears_once_the_project_is_saved() {
        let path = Path::new("/tmp/Night Drive.vzp");
        assert_eq!(window_title(Some(path), true), "vibez - Night Drive *");
        assert_eq!(window_title(Some(path), false), "vibez - Night Drive");
    }

    #[test]
    fn a_path_without_an_extension_still_titles_with_its_name() {
        assert_eq!(
            window_title(Some(Path::new("/tmp/scratch")), false),
            "vibez - scratch"
        );
    }

    #[test]
    fn a_clean_project_closes_without_a_prompt() {
        assert_eq!(close_request_decision(false), CloseRequest::Exit);
    }

    #[test]
    fn unsaved_changes_turn_a_close_request_into_a_confirmation() {
        assert_eq!(close_request_decision(true), CloseRequest::Confirm);
    }
}
