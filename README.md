# Mic Processor (`mic_filter`)

Real-time microphone conditioner for Windows. Captures your physical mic, runs a configurable DSP chain, and writes the result to a virtual cable so Discord / Zoom / browsers can use the cleaned signal as their microphone.

## Processing chain

The default chain preserves the original processing order:

1. **High-pass** (~80 Hz Butterworth) — optional rumble filter  
2. **RNNoise** (`nnnoiseless`) — neural noise suppression at 48 kHz / 480-sample frames  
3. **Downward expander** — quiets residual noise in pauses  
4. **Soft-knee compressor** — evening out speech dynamics  
5. **Gain** — output trim

In **Filter chain**, use **Add filter** to append a processor. Drag the dotted
grip on a filter card to reorder it; a colored line shows the insertion point.
The list scrolls when you hold a dragged filter near the viewport's top or bottom
edge. Release to apply the new order, or press **Escape** to cancel. You can also
use **Up** / **Down** to reorder and **Remove** to delete a filter.
Uncheck **Enabled** to bypass an instance
without losing its settings. Multiple instances of the same type are supported;
each has independent settings and DSP state. An empty chain passes audio through.

Edits apply at the next available audio block without restarting devices. Surviving
instances retain their state when reordered or adjusted. Click **Save** to persist
the chain. Mute always runs after the entire chain; monitor volume is separate.
Changes are not crossfaded, so abrupt gain/order changes can be audible.

Audio I/O uses **cpal** (WASAPI on Windows) with lock-free **ringbuf** queues and a dedicated DSP thread.

## Prerequisites

1. [Rust](https://rustup.rs/) (1.85+ recommended)
2. [VB-Audio Virtual Cable](https://vb-audio.com/Cable/) (free)

### Windows audio routing

1. Install VB-Cable and reboot if prompted.
2. Run this app and set **Output** to **CABLE Input** (auto-selected when detected).
3. In Windows Sound settings, set **CABLE Output** as the default *communication* device, **or** pick **CABLE Output** as the mic inside Discord/Zoom/etc.
4. Set **Input** in this app to your physical microphone.

> Do **not** set CABLE Output as the Windows default *playback* device, or you may get a feedback loop.

## Build & run

```powershell
cd C:\Users\erike\Rust\mic_filter
cargo run --release
```

A settings window and system-tray icon appear. Tray actions: Open settings, Start/Stop, Toggle mute, Toggle mic test, Quit.

### Mic test (monitor)

Use **Mic Test** to play the *filtered* signal to headphones/speakers so you can verify RNNoise and dynamics without opening Discord:

1. Start processing.
2. Choose a **monitor playback device** (headphones preferred — avoid speakers next to the mic).
3. Click **Apply monitor device** if you changed it.
4. Toggle **Mic Test ON** (toolbar, tray, or the Mic Test section).
5. Watch the **green / yellow / red** level bars while speaking.

| Zone | Range |
|------|--------|
| Green | below −12 dBFS |
| Yellow | −12 to −6 dBFS |
| Red | above −6 dBFS |

Monitor volume is independent of the VB-Cable output gain.

## Configuration

Persisted to:

`%APPDATA%\mic-processor\mic-processor\config.toml`

(via the `directories` crate project path)

| Setting | Meaning |
|--------|---------|
| `input_device` / `output_device` | Friendly device names |
| `filters` | Ordered list of independent filter instances |
| `filters[].id` | Stable instance identity (assigned automatically if omitted) |
| `filters[].type` | `high_pass`, `rnnoise`, `expander`, `compressor`, or `gain` |
| `filters[].enabled` | Bypass toggle for this instance |
| Other filter fields | `cutoff_hz`, `gain_db`, or dynamics parameters as appropriate |
| `start_processing` | Auto-start engine on launch |

Legacy configs are migrated automatically, preserving filter values, bypass flags,
and the original order. An explicit `filters = []` means no filters, not defaults.
New saves use the ordered format. For example:

```toml
[[filters]]
id = 1
type = "high_pass"
enabled = true
cutoff_hz = 80.0

[[filters]]
id = 2
type = "gain"
enabled = true
gain_db = -3.0
```

The UI publishes complete chain snapshots. DSP uses a non-blocking mutex read at
block boundaries; device callbacks do not access it. Filter construction and
reconfiguration happen on the dedicated DSP thread only when settings change.
Device changes need **Apply devices (restart)**.

## Latency & performance

- Target path: shared-mode WASAPI via cpal, 10 ms RNNoise frames, small ring priming (~20 ms).
- Expect roughly **30–50 ms** perceived latency depending on device buffer sizes.
- Underrun/overrun counters are shown in the UI for debugging.

## Project layout

```
src/
  main.rs          # startup + egui event loop
  ui.rs            # application/layout coordinator
  ui/
    state.rs       # shared application actions and settings state
    panels.rs      # toolbar, levels, monitor, devices, startup
    devices.rs     # reusable device selectors
    filters.rs     # ordered filter editor
    widgets.rs     # meters and atomic sliders
    tray.rs        # tray ownership, events, and icon
    tests.rs       # hardware-independent UI smoke tests
  config.rs        # app config and TOML persistence
  config/
    filters.rs     # typed filter settings and defaults
    legacy.rs      # backward-compatible loading and ID normalization
    live.rs        # shared DSP snapshots and global atomics
    tests.rs       # serialization and migration regression tests
  audio/
    devices.rs     # enumeration and device routing policy
    engine.rs      # stream lifecycle and worker ownership
    streams.rs     # format/config selection and callback construction
    samples.rs     # allocation-free PCM conversion and ring I/O
    processing.rs  # DSP worker, output sinks, buffered resampling
    meters.rs      # shared peak meters
  dsp/             # processors, common math, filter factory, ordered chain
plan.md
```

## Non-goals (v1)

- Acoustic echo cancellation
- VST hosting
- Cross-platform polish (Windows-first; cpal may work elsewhere but is untested)
- Automatic VB-Cable install

## Adding a new built-in filter

1. Add typed settings and a `FilterSettings` / `FilterKind` variant in `src/config/filters.rs`,
   including its label and defaults in `FilterKind::ALL`.
2. Implement `AudioFilter` (`configure` and in-place `process`, mono 48 kHz) and
   register construction in `src/dsp/filter.rs`. Processor state belongs to each
   instance. The chain handles ordering and bypass without filter-specific code.
3. Add parameter widgets to `src/ui/filters.rs` and DSP/config regression tests.

Run `cargo test` and `cargo clippy --all-targets` to validate changes.

## License

MIT
