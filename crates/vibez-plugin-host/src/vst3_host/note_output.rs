//! The VST3 output `IEventList`, collecting the notes a plugin outputs during one process call.

use vibez_core::note_output::NoteOutput;
use vst3::Steinberg::Vst::Event as VstEvent;
use vst3::Steinberg::Vst::Event_::EventTypes_::{kNoteOffEvent, kNoteOnEvent};

use super::abi::{
    static_com_add_ref, static_com_query_interface, static_com_release, vst_event, EventListVtbl,
};
use crate::output_note;

/// Lives on the stack for one `process` call; the plugin must not keep it.
#[repr(C)]
pub(super) struct Vst3NoteOutput<'a> {
    vtbl: *const EventListVtbl,
    notes: &'a mut NoteOutput,
    frames: u32,
}

impl<'a> Vst3NoteOutput<'a> {
    pub(super) fn new(notes: &'a mut NoteOutput, frames: u32) -> Self {
        Self {
            vtbl: &NOTE_OUTPUT_VTBL,
            notes,
            frames,
        }
    }

    pub(super) fn as_raw_mut(&mut self) -> *mut std::ffi::c_void {
        (self as *mut Self).cast()
    }

    /// False when the note was dropped.
    fn add(&mut self, event: &VstEvent) -> bool {
        let frame = u32::try_from(event.sampleOffset).unwrap_or(0);
        // SAFETY: the type tag selects the union field the plugin wrote.
        let note = if u32::from(event.r#type) == kNoteOnEvent {
            let on = unsafe { event.__field0.noteOn };
            output_note::note_on(frame, self.frames, on.pitch.into(), on.velocity.into())
        } else if u32::from(event.r#type) == kNoteOffEvent {
            let off = unsafe { event.__field0.noteOff };
            output_note::note_off(frame, self.frames, off.pitch.into())
        } else {
            None
        };
        note.is_none_or(|note| self.notes.push(note))
    }
}

unsafe extern "system" fn count(this: *mut std::ffi::c_void) -> i32 {
    let list = unsafe { &*this.cast::<Vst3NoteOutput<'_>>() };
    list.notes.notes().len() as i32
}

unsafe extern "system" fn get(
    this: *mut std::ffi::c_void,
    index: i32,
    event: *mut VstEvent,
) -> i32 {
    let list = unsafe { &*this.cast::<Vst3NoteOutput<'_>>() };
    let note = usize::try_from(index)
        .ok()
        .and_then(|index| list.notes.notes().get(index));
    match note {
        Some(note) if !event.is_null() => {
            unsafe { *event = vst_event(note) };
            0
        }
        _ => 1,
    }
}

/// Events other than notes (CC, sysex) are accepted and unused.
unsafe extern "system" fn add(this: *mut std::ffi::c_void, event: *mut VstEvent) -> i32 {
    let added = !event.is_null() && unsafe { (*this.cast::<Vst3NoteOutput<'_>>()).add(&*event) };
    if added {
        0
    } else {
        1
    }
}

static NOTE_OUTPUT_VTBL: EventListVtbl = EventListVtbl {
    query_interface: static_com_query_interface,
    add_ref: static_com_add_ref,
    release: static_com_release,
    get_event_count: count,
    get_event: get,
    add_event: add,
};

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_core::note_output::BlockNote;

    #[test]
    fn a_dropped_note_is_refused_to_the_plugin() {
        let mut output = NoteOutput::default();
        let mut list = Vst3NoteOutput::new(&mut output, 64);
        let raw = list.as_raw_mut();
        let mut on = vst_event(&BlockNote::on(3, 60, 100));
        let accepted = (0..NoteOutput::CAPACITY)
            .filter(|_| unsafe { (NOTE_OUTPUT_VTBL.add_event)(raw, &mut on) } == 0)
            .count();

        assert!(accepted < NoteOutput::CAPACITY);
        assert_eq!(accepted, list.notes.notes().len());
    }
}
