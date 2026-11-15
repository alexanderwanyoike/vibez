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

    /// # Safety
    /// `event` points to a whole event of at least `header.size` bytes, as
    /// CLAP guarantees for `try_push`.
    unsafe fn push(&mut self, event: *const clap_event_header) -> bool {
        let header = &*event;
        if header.space_id != CLAP_CORE_EVENT_SPACE_ID {
            return true;
        }
        let note = match header.type_ {
            CLAP_EVENT_NOTE_ON | CLAP_EVENT_NOTE_OFF if fits::<clap_event_note>(header) => {
                let note = &*event.cast::<clap_event_note>();
                if header.type_ == CLAP_EVENT_NOTE_ON {
                    output_note::note_on(header.time, self.frames, note.key.into(), note.velocity)
                } else {
                    output_note::note_off(header.time, self.frames, note.key.into())
                }
            }
            CLAP_EVENT_MIDI if fits::<clap_event_midi>(header) => {
                let midi = &*event.cast::<clap_event_midi>();
                output_note::midi(header.time, self.frames, midi.data)
            }
            _ => None,
        };
        note.is_none_or(|note| self.notes.push(note))
    }
}

fn fits<T>(header: &clap_event_header) -> bool {
    header.size as usize >= std::mem::size_of::<T>()
}

/// Events other than notes (parameter gestures, CC) are accepted and unused.
unsafe extern "C" fn try_push(
    list: *const clap_output_events,
    event: *const clap_event_header,
) -> bool {
    if list.is_null() || event.is_null() {
        return false;
    }
    (*(*list).ctx.cast::<ClapNoteOutput<'_>>()).push(event)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(type_: u16, size: u32) -> clap_event_note {
        clap_event_note {
            header: clap_event_header {
                size,
                time: 3,
                space_id: CLAP_CORE_EVENT_SPACE_ID,
                type_,
                flags: 0,
            },
            note_id: -1,
            port_index: 0,
            channel: 0,
            key: 60,
            velocity: 1.0,
        }
    }

    fn push(output: &mut NoteOutput, event: &clap_event_note) -> bool {
        let mut list = ClapNoteOutput::new(output, 64);
        let events = list.events();
        unsafe { (events.try_push.unwrap())(&events, (event as *const clap_event_note).cast()) }
    }

    #[test]
    fn an_event_smaller_than_its_type_is_ignored() {
        let mut output = NoteOutput::default();
        let header_only = std::mem::size_of::<clap_event_header>() as u32;

        push(&mut output, &note(CLAP_EVENT_NOTE_ON, header_only));

        assert!(output.notes().is_empty());
    }

    #[test]
    fn a_dropped_note_is_refused_to_the_plugin() {
        let mut output = NoteOutput::default();
        let on = note(
            CLAP_EVENT_NOTE_ON,
            std::mem::size_of::<clap_event_note>() as u32,
        );
        let accepted = (0..NoteOutput::CAPACITY)
            .filter(|_| push(&mut output, &on))
            .count();

        assert_eq!(accepted, output.notes().len());
    }
}
