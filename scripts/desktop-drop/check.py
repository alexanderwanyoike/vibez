"""Checks the project Vibez auto-saved after the drags.

The drags put the MIDI file on empty Arrange and then on the lane it made,
and the WAV file on the new-track row and then on the lane it made. So the
saved project holds a MIDI track with two note Clips and an audio track with
two audio Clips.
"""

import json
import sqlite3
import sys
import time
from pathlib import Path


def load(path: Path) -> dict:
    connection = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    try:
        (document,) = connection.execute(
            "SELECT json FROM project_document WHERE singleton = 1"
        ).fetchone()
    finally:
        connection.close()
    return json.loads(document)["project"]


def summary(project: dict) -> list:
    return [
        {
            "track": track["name"],
            "kind": track["kind"],
            "note_clips": sorted(
                clip["position_beats"]
                for clip in project.get("note_clips", [])
                if clip["track_id"] == track["id"]
            ),
            "audio_clips": sum(
                1 for clip in project.get("clips", []) if clip["track_id"] == track["id"]
            ),
        }
        for track in project["tracks"]
    ]


def landed(tracks: list) -> bool:
    kinds = [(track["kind"], len(track["note_clips"]), track["audio_clips"]) for track in tracks]
    return kinds == [("Midi", 2, 0), ("Audio", 0, 2)]


def main() -> int:
    path = Path(sys.argv[1])
    # Auto-save waits two seconds after the last edit, then writes.
    tracks = []
    for _ in range(30):
        try:
            tracks = summary(load(path))
        except sqlite3.OperationalError:
            # A save in flight can hold the file for a moment.
            tracks = []
        if landed(tracks):
            print(json.dumps(tracks, indent=2))
            return 0
        time.sleep(1)
    print("The drops did not all land:", json.dumps(tracks, indent=2), file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
