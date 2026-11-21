"""Writes the files the desktop drop check drags, and Vibez's settings.

A one-bar drum phrase as a MIDI file and a one-second tone as a WAV file go
in the drop folder. The settings close the Browser, so Arrange gets the room,
set the interface scale and skip the update check.

Usage: fixtures.py <drop folder> <settings folder> <interface scale>
"""

import json
import math
import struct
import sys
import wave
from pathlib import Path


def write_settings(folder: Path, interface_scale: float) -> None:
    settings = {
        "sample_library_roots": [],
        "sample_browser_open": False,
        "check_for_updates": False,
        "interface_scale": interface_scale,
    }
    folder.mkdir(parents=True, exist_ok=True)
    (folder / "ui.json").write_text(json.dumps(settings))


def write_midi(path: Path) -> None:
    """A type 0 file at 96 PPQ: kick on 1, snare on 2, one bar long."""
    events = bytes(
        [
            0x00, 0x99, 36, 110, 24, 0x89, 36, 0,
            72, 0x99, 38, 110, 24, 0x89, 38, 0,
            0x82, 0x08, 0xFF, 0x2F, 0x00,
        ]
    )
    header = b"MThd" + struct.pack(">IHHH", 6, 0, 1, 96)
    track = b"MTrk" + struct.pack(">I", len(events)) + events
    path.write_bytes(header + track)


def write_wav(path: Path) -> None:
    """One second of a quiet 220 Hz sine, 16-bit mono at 48 kHz."""
    rate = 48000
    frames = b"".join(
        struct.pack("<h", int(8000 * math.sin(2 * math.pi * 220 * n / rate)))
        for n in range(rate)
    )
    with wave.open(str(path), "wb") as output:
        output.setnchannels(1)
        output.setsampwidth(2)
        output.setframerate(rate)
        output.writeframes(frames)


def main() -> None:
    drop = Path(sys.argv[1])
    drop.mkdir(parents=True, exist_ok=True)
    write_midi(drop / "drums.mid")
    write_wav(drop / "tone.wav")
    write_settings(Path(sys.argv[2]), float(sys.argv[3]))


if __name__ == "__main__":
    main()
