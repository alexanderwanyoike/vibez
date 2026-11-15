//! A file another app drags over the window, carried as the Browser drag in
//! flight until its drop lands on a target or misses.

use std::path::PathBuf;

use super::{BrowserAction, DragMedia};
use crate::state::{BrowserState, DesktopDrop};
use vibez_core::track::MediaSourceRef;

impl BrowserState {
    /// Whether the drag in flight is a file from another app.
    pub fn drags_from_desktop(&self) -> bool {
        self.desktop_drop.is_some()
    }

    /// Whether a desktop drop waits for the pointer to report a target.
    pub fn desktop_drop_waits_for_target(&self) -> bool {
        self.desktop_drop == Some(DesktopDrop::Dropped)
    }

    /// Makes a file another app drags over the window the drag in flight;
    /// only the first of several files is taken.
    pub(super) fn hover_desktop_file(&mut self, path: PathBuf) -> Option<String> {
        if self.drag_source.is_some() && !self.desktop_drop_waits_for_target() {
            return None;
        }
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        if !vibez_core::audio_format::is_supported_audio_path(&path)
            && !vibez_core::midi_file::is_midi_file_path(&path)
        {
            return Some(format!("Vibez can't import {name}"));
        }
        self.cancel_media_drag();
        self.drag_source = Some(MediaSourceRef::LocalFile { path });
        self.drag_label = Some(name);
        self.desktop_drop = Some(DesktopDrop::Hovering);
        Some(self.moving_status())
    }

    /// Releases the dropped file on the target under the pointer, cancels
    /// it when the pointer showed nothing there, or waits for a target.
    pub(super) fn drop_desktop_file(&mut self, path: &std::path::Path, action: &mut BrowserAction) {
        let dragged = matches!(
            &self.drag_source,
            Some(MediaSourceRef::LocalFile { path: dragged }) if dragged == path
        );
        let over_window = matches!(
            self.desktop_drop,
            Some(DesktopDrop::Hovering | DesktopDrop::PointerSeen)
        );
        if !dragged || !over_window {
            return;
        }
        match self.drag_target.clone() {
            Some(target) => self.release_desktop_drop(target, action),
            None if self.desktop_drop == Some(DesktopDrop::PointerSeen) => {
                self.miss_desktop_drop(action);
            }
            None => self.desktop_drop = Some(DesktopDrop::Dropped),
        }
    }

    pub(super) fn release_desktop_drop(
        &mut self,
        target: crate::state::BrowserDropTarget,
        action: &mut BrowserAction,
    ) {
        if target.accepts_drop() {
            action.release = Some(target);
            return;
        }
        let refusal = target.status(self.drag_media().unwrap_or(DragMedia::Audio));
        self.cancel_media_drag();
        action.status = Some(refusal);
    }

    pub(super) fn miss_desktop_drop(&mut self, action: &mut BrowserAction) {
        let name = self.drag_label.clone().unwrap_or_default();
        self.cancel_media_drag();
        action.status = Some(format!("Nothing here takes {name}"));
    }

    /// Ends the desktop phase of a drop being released on its target, so the
    /// release is not taken for a click.
    pub fn end_desktop_drop(&mut self) {
        self.desktop_drop = None;
    }
}
