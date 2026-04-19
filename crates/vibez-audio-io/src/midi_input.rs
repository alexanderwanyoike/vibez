//! External MIDI input support via [`midir`].
//!
//! Exposes a tiny API over the cross-platform midir crate:
//!
//! - [`list_midi_input_ports`] returns the names of available MIDI
//!   inputs the OS is advertising (USB keyboards, virtual MIDI cables,
//!   Ableton Push, etc.).
//! - [`open_midi_input`] connects to a named port and returns a
//!   [`MidiInputHandle`] whose `rx` channel yields parsed
//!   [`MidiEvent`]s from a midir-managed background thread.
//!
//! The parser handles the handful of status bytes the DAW cares about
//! right now: Note On / Note Off (treating velocity-0 Note On as Note
//! Off, as most hardware does) and Control Change. Aftertouch, pitch
//! bend, and SysEx are currently ignored.
//!
//! Consumers (the UI layer) drain the receiver on every frame tick and
//! forward the events to the engine as `EngineCommand::ExternalNoteOn` /
//! `ExternalNoteOff`.

use std::sync::mpsc::{self, Receiver};

use midir::{Ignore, MidiInput, MidiInputConnection};

/// A parsed, high-level MIDI event the DAW acts on.
#[derive(Debug, Clone, Copy)]
pub enum MidiEvent {
    /// Note pressed. Velocity 1..=127 — velocity 0 is normalised to
    /// `NoteOff` by the parser because that is how most hardware
    /// encodes note-off.
    NoteOn { pitch: u8, velocity: u8 },
    NoteOff { pitch: u8 },
    ControlChange { cc: u8, value: u8 },
}

/// Active MIDI input connection. Dropping this closes the port and
/// stops the background listener thread.
pub struct MidiInputHandle {
    // Keep the connection alive for the handle's lifetime. midir tears
    // down the listener thread on drop.
    _conn: MidiInputConnection<()>,
    pub port_name: String,
    pub rx: Receiver<MidiEvent>,
}

/// Enumerate MIDI input ports visible to the OS right now. The names
/// returned are what should be passed to [`open_midi_input`].
pub fn list_midi_input_ports() -> Result<Vec<String>, String> {
    let input = MidiInput::new("vibez-midi-scan").map_err(|e| e.to_string())?;
    let ports = input.ports();
    let mut names = Vec::with_capacity(ports.len());
    for port in ports {
        if let Ok(name) = input.port_name(&port) {
            names.push(name);
        }
    }
    Ok(names)
}

/// Open the MIDI input port whose name matches `target_name`. The
/// callback converts raw MIDI bytes into [`MidiEvent`]s and forwards
/// them over the returned channel. The caller keeps the handle alive
/// for as long as it wants to receive events.
pub fn open_midi_input(target_name: &str) -> Result<MidiInputHandle, String> {
    let mut input = MidiInput::new("vibez-midi-in").map_err(|e| e.to_string())?;
    // Ignore SysEx, time, and active-sensing clutter: keeps the
    // parser focused on playing notes.
    input.ignore(Ignore::All);

    let ports = input.ports();
    let port = ports
        .into_iter()
        .find(|p| {
            input
                .port_name(p)
                .map(|name| name == target_name)
                .unwrap_or(false)
        })
        .ok_or_else(|| format!("MIDI port not found: {target_name}"))?;

    let resolved_name = input.port_name(&port).unwrap_or_else(|_| target_name.to_string());
    let (tx, rx) = mpsc::channel::<MidiEvent>();

    let conn = input
        .connect(
            &port,
            "vibez-midi-listener",
            move |_timestamp, message, _user_data| {
                if let Some(event) = parse_midi_message(message) {
                    // If the receiver is gone the send silently fails;
                    // the listener thread exits on next drop.
                    let _ = tx.send(event);
                }
            },
            (),
        )
        .map_err(|e| e.to_string())?;

    Ok(MidiInputHandle {
        _conn: conn,
        port_name: resolved_name,
        rx,
    })
}

fn parse_midi_message(message: &[u8]) -> Option<MidiEvent> {
    if message.is_empty() {
        return None;
    }
    let status = message[0] & 0xF0;
    match status {
        0x90 => {
            // Note On. Vel 0 → Note Off, per convention.
            let pitch = *message.get(1)?;
            let velocity = *message.get(2)?;
            if velocity == 0 {
                Some(MidiEvent::NoteOff { pitch })
            } else {
                Some(MidiEvent::NoteOn { pitch, velocity })
            }
        }
        0x80 => {
            let pitch = *message.get(1)?;
            Some(MidiEvent::NoteOff { pitch })
        }
        0xB0 => {
            let cc = *message.get(1)?;
            let value = *message.get(2)?;
            Some(MidiEvent::ControlChange { cc, value })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_note_on() {
        let ev = parse_midi_message(&[0x90, 60, 100]);
        assert!(matches!(
            ev,
            Some(MidiEvent::NoteOn {
                pitch: 60,
                velocity: 100
            })
        ));
    }

    #[test]
    fn parses_note_on_velocity_zero_as_note_off() {
        let ev = parse_midi_message(&[0x90, 60, 0]);
        assert!(matches!(ev, Some(MidiEvent::NoteOff { pitch: 60 })));
    }

    #[test]
    fn parses_note_off() {
        let ev = parse_midi_message(&[0x80, 62, 40]);
        assert!(matches!(ev, Some(MidiEvent::NoteOff { pitch: 62 })));
    }

    #[test]
    fn parses_cc() {
        let ev = parse_midi_message(&[0xB0, 7, 127]);
        assert!(matches!(
            ev,
            Some(MidiEvent::ControlChange { cc: 7, value: 127 })
        ));
    }

    #[test]
    fn parses_channel_variants() {
        // Channel 4 note-on should parse the same as channel 1.
        let ev = parse_midi_message(&[0x93, 64, 80]);
        assert!(matches!(
            ev,
            Some(MidiEvent::NoteOn {
                pitch: 64,
                velocity: 80
            })
        ));
    }

    #[test]
    fn ignores_unknown_status() {
        assert!(parse_midi_message(&[0xF0, 1, 2, 3]).is_none());
        assert!(parse_midi_message(&[]).is_none());
    }

    #[test]
    fn list_ports_never_panics() {
        // May legitimately return Err on CI without an ALSA seq / core
        // MIDI service. The important bit is that it doesn't panic.
        let _ = list_midi_input_ports();
    }
}
