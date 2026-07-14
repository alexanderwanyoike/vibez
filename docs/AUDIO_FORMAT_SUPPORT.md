# Audio format support

Vibez uses one import contract for Local Browser results, Dropbox Remote
Catalog eligibility, native audio pickers, Audition, Arrange/device import,
and Project Media reopen.

The executable source of truth is
`vibez_core::audio_format::SUPPORTED_AUDIO_FORMATS`. The Symphonia features in
the workspace `Cargo.toml` must match this matrix, and fixture tests in
`vibez-audio-io/tests/audio_format_matrix.rs` prove the shipped decoder rather
than assuming that an extension is sufficient.

| UI label | Extensions | Container | Supported codec |
| --- | --- | --- | --- |
| WAV | `.wav`, `.wave` | RIFF/WAVE | PCM |
| AIFF | `.aif`, `.aiff` | AIFF | PCM |
| FLAC | `.flac` | FLAC | FLAC |
| MP3 | `.mp3` | MPEG audio | MPEG Layer III |
| OGG | `.ogg` | Ogg | Vorbis |
| M4A | `.m4a` | ISO MP4/M4A | AAC |

An extension only makes a Source Entry eligible for decoding. Vibez probes and
decodes the actual bytes before Audition or import. Unsupported containers,
unsupported codecs inside a recognized container, empty audio, and corrupt
input return a recoverable error before a clip or Project Media reference is
created.

Raw `.aac`/ADTS is not advertised because the shipped stack has an AAC decoder
but no supported ADTS demuxer. ALAC-in-M4A, Opus-in-Ogg, MIDI, archive members,
and export/conversion formats are outside this V1 matrix.

Decoded metadata is authoritative: channel count and sample rate come from the
decoded signal, and duration comes from decoded frames. Browser rows may show
unknown metadata until the source has been materialized and decoded.
