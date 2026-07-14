# Audio format fixtures

These are synthetic 0.25-second sine tones generated for Vibez; they contain
no third-party music or sample content.

- `mono-44100-s16.wav`: mono, 44.1 kHz, 16-bit PCM
- `stereo-48000-s24.aiff`: stereo, 48 kHz, 24-bit PCM
- `mono-32000-s24.flac`: mono, 32 kHz, 24-bit FLAC
- `stereo-44100.mp3`: stereo, 44.1 kHz, 128 kbps MP3
- `mono-48000.ogg`: mono, 48 kHz, Ogg Vorbis
- `stereo-44100.m4a`: stereo, 44.1 kHz, 128 kbps AAC in M4A

The matrix tests intentionally validate decoded channel count, sample rate,
duration, and non-silent playback rather than trusting these filenames.
