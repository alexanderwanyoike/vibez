"""Writes what the desktop drop check drags and drops into.

An empty project for Vibez to open and auto-save, a one-bar drum phrase as
a MIDI file and a one-second tone as a WAV file, all under the given folder.
"""

import json
import math
import sqlite3
import struct
import sys
import wave
from pathlib import Path

# Project Format V1, as crates/vibez-project/src/project_format_v1.rs
# defines it: "VZP1" in the application id and the document as JSON.
APPLICATION_ID = 0x565A5031
FORMAT_VERSION = 1
SCHEMA = """
CREATE TABLE project_document (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    json BLOB NOT NULL
);
CREATE TABLE project_media (
    id TEXT PRIMARY KEY,
    file_name TEXT NOT NULL,
    byte_len INTEGER NOT NULL,
    sha256 TEXT NOT NULL,
    provenance_json BLOB NOT NULL,
    content BLOB NOT NULL
);
"""


def write_project(path: Path) -> None:
    document = {
        "format_version": FORMAT_VERSION,
        "project": {
            "name": "Desktop drop",
            "bpm": 120.0,
            "sample_rate": 48000,
            "tracks": [],
        },
        "project_media": [],
    }
    path.unlink(missing_ok=True)
    connection = sqlite3.connect(path)
    connection.executescript(SCHEMA)
    connection.execute(f"PRAGMA application_id = {APPLICATION_ID}")
    connection.execute(f"PRAGMA user_version = {FORMAT_VERSION}")
    connection.execute(
        "INSERT INTO project_document(singleton, json) VALUES (1, ?)",
        (json.dumps(document).encode(),),
    )
    connection.commit()
    connection.close()


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
    folder = Path(sys.argv[1])
    folder.mkdir(parents=True, exist_ok=True)
    write_project(folder / "project.vzp")
    write_midi(folder / "drums.mid")
    write_wav(folder / "tone.wav")


if __name__ == "__main__":
    main()
