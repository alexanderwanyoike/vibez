//! The CLAP output event list, collecting the notes a plugin outputs during one process call.

use clap_sys::events::{
    clap_event_header, clap_event_midi, clap_event_note, clap_output_events,
    CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_MIDI, CLAP_EVENT_NOTE_OFF, CLAP_EVENT_NOTE_ON,
};
use vibez_core::note_output::NoteOutput;

use crate::output_note;

/// Lives on the stack for one `process` call; the plugin must not keep it.
pub(super) struct ClapNoteOutput<'a> {
    notes: &'a mut NoteOutput,
    frames: u32,
}

impl<'a> ClapNoteOutput<'a> {
    pub(super) fn new(notes: &'a mut NoteOutput, frames: u32) -> Self {
        Self { notes, frames }
    }

    /// Valid while `self` stays in place, which `process` guarantees by
    /// holding it across the plugin call.
    pub(super) fn events(&mut self) -> clap_output_events {
        clap_output_events {
            ctx: (self as *mut Self).cast(),
            try_push: Some(try_push),
        }
    }

    unsafe fn push(&mut self, header: &clap_event_header) {
        if header.space_id != CLAP_CORE_EVENT_SPACE_ID {
            return;
        }
        let note = match header.type_ {
            CLAP_EVENT_NOTE_ON => {
                let event = &*(header as *const clap_event_header).cast::<clap_event_note>();
                output_note::note_on(header.time, self.frames, event.key.into(), event.velocity)
            }
            CLAP_EVENT_NOTE_OFF => {
                let event = &*(header as *const clap_event_header).cast::<clap_event_note>();
                output_note::note_off(header.time, self.frames, event.key.into())
            }
            CLAP_EVENT_MIDI => {
                let event = &*(header as *const clap_event_header).cast::<clap_event_midi>();
                output_note::midi(header.time, self.frames, event.data)
            }
            _ => None,
        };
        if let Some(note) = note {
            self.notes.push(note);
        }
    }
}

/// Every event is accepted: notes are kept or counted as dropped, and other
/// output (parameter gestures, CC) is not used yet.
unsafe extern "C" fn try_push(
    list: *const clap_output_events,
    event: *const clap_event_header,
) -> bool {
    if !list.is_null() && !event.is_null() {
        (*(*list).ctx.cast::<ClapNoteOutput<'_>>()).push(&*event);
    }
    true
}
