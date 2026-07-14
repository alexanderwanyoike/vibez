# Project Format V1 SQLite container proof

Date: 2026-07-13  
Scope: Browser Card 01 technical proof only

## Result

Proceed with a SQLite-backed `.vzp` Project Container, with changes before it
becomes the production persistence path. The proof preserved representative
project state and 64 MiB of Project Media, performed a document-only save
without touching media rows, produced an independent Save As container, and
recovered the last committed document and media after a child process exited
mid-transaction.

The existing JSON production save/load path remains unchanged.

## Repeatable fixture

Run:

```sh
cargo run --release -p vibez-project --bin project-format-v1-proof -- /tmp/vibez-card-01-measurement
```

The deterministic fixture contains:

- two valid 32 MiB stereo 48 kHz/16-bit PCM WAV assets, one with Local
  provenance and one with Dropbox Remote provenance;
- a versioned project document with MIDI notes and loop state;
- track automation;
- native-device parameter state;
- opaque third-party plugin state;
- master and return channel state; and
- a Project Media manifest linking owned bytes to provenance.

First save ingests the two Staged Media files transactionally and removes the
staging copies only after commit. Save As uses SQLite `VACUUM INTO` to make an
independent, compact container. The interruption case launches the same proof
executable as a child, updates the document and media inside a transaction,
and exits with code 86 without commit or Rust destructor cleanup.

## Measurements

Measured on Linux 6.8.0-134-generic, ext4 on NVMe, an Intel i5-11400H, and
Rust 1.96.1. Timings are one local observation, not a performance budget.

| Observation | Result |
|---|---:|
| Fixture media | 67,108,864 bytes |
| Full save | 214.218 ms |
| Document-only incremental save | 6.140 ms |
| Normal reopen and document read | 0.091 ms |
| Save As | 76.931 ms |
| Reopen after interrupted transaction | 7.325 ms |
| Container after full save | 67,190,784 bytes |
| Container after incremental save and recovery | 67,190,784 bytes |
| Media rows written by incremental save | 0 |
| Media bytes written by incremental save | 0 |

The unchanged media row retained rowid `1`, byte length, and SHA-256
`f734805f2f179ae439a9f1e28d0be4ae498e930b24db7ae1f111cc38a7f9983a`
before and after the document-only save. The container size did not grow.

Both original and Save As files were recognized as SQLite 3 databases with
application id `0x565a5031` (`VZP1`) and user version `1`. Their first 16 bytes
are `SQLite format 3\0`, not ZIP's `PK` signature. No SQLite journal or other
sidecar remained after commit or recovery.

## Automated evidence

`cargo test -p vibez-project` passes 17 tests: 14 existing JSON-path tests and
3 proof integration tests. The proof tests cover:

- complete document, media, and provenance round-trip;
- Staged Media ownership on first save;
- zero media-row/media-byte writes on a document-only save;
- stable media rowid, byte length, and SHA-256 across that save;
- Save As independence;
- explicit SQLite application/document version markers and non-ZIP signature;
  and
- child-process interruption followed by unchanged committed document and
  media recovery.

## Failure modes and trade-offs

- The proof reads each staged asset into memory before inserting it. Production
  implementation should use SQLite incremental BLOB I/O (or an equivalently
  bounded streaming path) and expose progress/cancellation for multi-gigabyte
  media.
- SQLite's rollback-journal mode creates a transient sidecar while a write is
  active. The `.vzp` is the sole durable artifact after commit/recovery, but
  production must define same-directory filesystem requirements and test
  power loss, disk-full, and filesystems with weaker durability semantics.
- Staging cleanup is best-effort after commit. Production needs a persisted
  cleanup queue so committed Project Media is never removed while abandoned
  staging copies are eventually reclaimed.
- A failed first-save proof can leave an incomplete destination file.
  Production should create under a private temporary name and atomically
  publish the first committed container.
- SHA-256 is recorded and tested, but reads do not automatically verify it.
  Production should define when integrity verification runs and how corruption
  is reported/recovered.
- `VACUUM INTO` is safe and compact but copies all bytes, so Save As cost scales
  with total Project Media. That is acceptable semantics, but UI progress and
  disk-space preflight are required.
- The benchmark uses deterministic PCM WAV fixtures without decoding them and
  does not establish cross-platform latency or maximum supported project size.

## Recommendation

Proceed with SQLite as the Project Format V1 container foundation. Before Card
03 turns the proof into production persistence, change media ingestion to a
bounded streaming implementation and specify first-save publication, staging
cleanup, integrity checks, disk-full behavior, and cross-platform crash/power-
loss validation. Preserve the proof's explicit `VZP1` application id, format
version, transactional document/media update, and independent Save As model.
