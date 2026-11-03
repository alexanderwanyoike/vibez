# vibez-mcp

A [Model Context Protocol](https://modelcontextprotocol.io) server that lets an
AI agent build tracks with Vibez: find samples, create a project with native
instruments and effects, place audio and note clips, bounce it to a WAV through
the Vibez engine, and save a `.vzp` the app opens.

It works on Vibez's own types. Projects are `vibez_project::Project` values,
device parameters come from the same descriptor tables the app's devices use,
bounces build the same `BounceRequest` as **Export**, and saves go through the
same `.vzp` writer. Anything the server makes, the app can open and edit.

## Running it

```sh
cargo build --release -p vibez-mcp
```

The binary speaks MCP over stdio. To register it with Claude Code:

```sh
claude mcp add vibez -- /path/to/vibez/target/release/vibez-mcp
```

Any MCP client that launches stdio servers works the same way.

### Configuration

| Setting | Default | Purpose |
|---|---|---|
| `VIBEZ_MCP_SAMPLE_DIRS` | `~/Music/Samples` | Local sample folders, separated like `PATH` |
| Dropbox | the app's connection | Search uses the Browser's Dropbox catalog; fetching uses the app's tokens and media cache |

Dropbox needs no setup beyond connecting it once in the app's Browser. The
server reads the same catalog and cache, so samples fetched by either side are
shared, and refreshed tokens are written back for the app.

## Tools

| Tool | What it does |
|---|---|
| `search_samples` | Find audio files whose path contains every query word, locally and in Dropbox |
| `fetch_sample` | Download a Dropbox hit into the media cache and return its local path |
| `inspect_sample` | Length, channels, sample rate, peak and Vibez's tempo estimate |
| `list_device_parameters` | A native instrument's or effect's parameters with ranges and defaults |
| `create_project` / `open_project` | Start a project, or open a `.vzp` or legacy JSON project |
| `describe_project` | Tempo, length, tracks, devices and clip counts |
| `set_tempo` | Change the project tempo |
| `add_track` | An audio track, or a MIDI track with `subtractive_synth`, `wavetable_synth`, `fm_synth` or `sampler` |
| `set_track_mix` | Volume in dB, pan from -1 to 1, mute |
| `set_instrument_parameters` | Named instrument parameters, clamped to their ranges |
| `add_effect` / `set_effect_parameters` | Native effects on a track; track 0 is the master |
| `add_audio_clip` | Place a sample; `loop_beats` repeats its start at the project tempo |
| `add_note_clip` | Place MIDI notes on an instrument track |
| `bounce` | Render to WAV through the engine, with an optional beat range and tail |
| `save_project` | Save a self-contained `.vzp` with every sample embedded |

Timing is in beats from the start of the project, four beats to a bar. Note
start beats are relative to their clip.

## A typical session

1. `search_samples` for material, `inspect_sample` to check tempo and length,
   and `fetch_sample` for anything that lives in Dropbox.
2. `create_project` at the tempo of the loops you picked.
3. `add_track` for each part, `list_device_parameters` to learn a device, then
   `set_instrument_parameters` and `add_effect` to shape it.
4. `add_audio_clip` and `add_note_clip` to arrange.
5. `bounce` to listen and check levels in the report, adjust, repeat.
6. `save_project` and open the result in Vibez to keep working by hand.

## Limits

- Audio clips play at their recorded speed. Warp is not applied yet, so loops
  should be chosen or cut to match the project tempo (`inspect_sample` helps).
- Plugin devices are not offered, and bouncing an opened project that uses
  plugins fails as incomplete rather than writing a WAV without them.
- One project is open per server process.
