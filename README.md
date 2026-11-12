# Mic Processor (`mic_filter`)

Real-time microphone conditioner for Windows. Captures your physical mic, runs a configurable DSP chain, and writes the result to a virtual cable so Discord / Zoom / browsers can use the cleaned signal as their microphone.

## Processing chain

The default chain preserves the original processing order:

1. **High-pass** (~80 Hz Butterworth) — optional rumble filter  
2. **RNNoise** (`nnnoiseless`) — neural noise suppression at 48 kHz / 480-sample frames  
3. **Downward expander** — quiets residual noise in pauses  
4. **Soft-knee compressor** — evening out speech dynamics  
5. **Gain** — output trim

A separate optional **final-stage peak limiter** runs after every filter and
before mute. It is disabled by default to preserve existing setups.

In **Filter chain**, use **Add filter** to append a processor. Drag the dotted
grip on a filter card to reorder it; a colored line shows the insertion point.
The list scrolls when you hold a dragged filter near the viewport's top or bottom
edge. Release to apply the new order, or press **Escape** to cancel. You can also
use **Up** / **Down** to reorder and **Remove** to delete a filter.
Uncheck **Enabled** to bypass an instance
without losing its settings. Multiple instances of the same type are supported;
each has independent settings and DSP state. An empty chain passes audio through
unless the final limiter or mute is enabled.

Edits apply at the next available audio block without restarting devices. Surviving
instances retain their state when reordered or adjusted. Click **Save** to persist
the chain. Mute always runs after the entire chain; monitor volume is separate.
Changes are not crossfaded, so abrupt gain/order/profile changes can be audible.

### Final-stage peak limiter

Enable **Final-stage peak limiter** below the filter chain to contain sample
peaks, including sudden shouts and compressor makeup gain. Set:

- **Ceiling**: −24 to 0 dBFS (initial value −1 dBFS).
- **Release**: 10–1000 ms (initial value 100 ms).
- **Gain reduction**: live attenuation is displayed in dB with a meter; replay
  has an independent attenuation readout and DSP state.

The limiter uses instantaneous attack and smoothed recovery, with no lookahead
or added buffering. It always runs last, cannot be reordered, and is saved
with the chain. Click **Save** to persist edits, or store them in a named profile.

This is **48 kHz sample-peak limiting, not true-peak limiting**. It cannot undo
clipping in the mic or earlier filters (including the Gain filter's existing
full-scale clamp). Later resampling or positive monitor gain can produce peaks
above the ceiling. Original replay comparison bypasses all DSP, including the
limiter; processed replay includes it.

### Named profiles

Use **Named profiles** to keep setups such as Meetings, Gaming, or Streaming:

- Enter a name and **Save as new** to snapshot the working chain.
- Select a saved profile and **Load selected** to apply it live, without
  restarting devices. Fresh processor state is used on load.
- **Overwrite selected** explicitly replaces its snapshot with current settings.
- **Rename** / **Duplicate** use the name entered in the text box. Duplicate
  copies the selected saved snapshot, not any unsaved working edits.
- **Delete…** requires confirmation and leaves the working chain unchanged.
- The tray's **Profiles (replaces current settings)** submenu loads saved
  profiles directly.

Profiles store filter order, parameters, bypass states, and the final limiter.
They do not change devices, monitor settings, mute, or tuning recordings.
Loading from the window or tray replaces working edits; save a new profile first
if you want to retain them. The panel indicates whether the selected saved profile
matches the current settings. Toolbar **Save** persists working settings but does
not overwrite named snapshots. Profile management and loads save automatically
to the same `config.toml`; any disk-write error is displayed in the status area.

For sharing, expand **Import / export profile TOML**, select a saved profile,
click **Export selected**, then **Copy TOML**. Paste that document into another
instance and **Import as new**. Import does not automatically load the profile.
Imports use a versioned TOML document and reject invalid DSP ranges, non-finite
values, duplicate names (case-insensitive), and documents over 1 MiB. Names are
limited to 80 characters, with up to 128 profiles and 64 filters per profile.

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

A settings window and system-tray icon appear. Tray actions: Open settings,
Start/Stop, Toggle mute, Toggle mic test, Profiles, Quit.

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

### Record-and-replay tuning

Use **Record & replay tuning** to adjust filters against the same speech:

1. Start processing and select/apply a headphone monitor device.
2. Click **Record dry mic**, speak normally, then **Stop recording / replay**.
   Recording stops automatically at 30 seconds. It captures 48 kHz mono audio
   before all filters and mute; muting the output does not stop recording.
3. Click **Loop in headphones**. This enables Mic Test and loops the clip through
   a separate copy of the current filter chain. Edit filters as usual to hear
   their effect without speaking again.
4. Switch **Original** / **Processed** to compare. Optional approximate RMS
   matching attenuates the louder signal with roughly 500 ms smoothing; it does
   not boost quiet audio, measure perceptual loudness, or prevent clipping.
5. Stop replay to return to live monitoring. **Record new clip** replaces the
   previous recording; **Clear clip** discards it.

**Replay is monitor-only and is never sent to the virtual cable. The cable keeps
transmitting your live, filtered microphone during tuning.** Global mute silences
both outputs, and monitor volume remains independent. Use headphones, not speakers.
The level meters continue to show the live input/cable signal, not replay.

Clips are held only in memory (about 5.8 MB maximum), not saved or exported.
Stopping/restarting devices stops recording/replay but retains the clip until you
clear it or quit. Replay is paced by incoming mic blocks, so capture must remain
active. Filter state persists across loop boundaries; edits and loop seams are
not crossfaded and can be audible.

## Configuration

Persisted to:

`%APPDATA%\mic-processor\mic-processor\config.toml`

(via the `directories` crate project path)

| Setting | Meaning |
|--------|---------|
| `input_device` / `output_device` | Friendly device names |
| `filters` | Ordered list of independent filter instances |
| `limiter` | Final-stage `enabled`, `ceiling_db`, and `release_ms` |
| `profiles` | Named snapshots containing `name`, `filters`, and `limiter` |
| `filters[].id` | Stable instance identity (assigned automatically if omitted) |
| `filters[].type` | `high_pass`, `rnnoise`, `expander`, `compressor`, or `gain` |
| `filters[].enabled` | Bypass toggle for this instance |
| Other filter fields | `cutoff_hz`, `gain_db`, or dynamics parameters as appropriate |
| `start_processing` | Auto-start engine on launch |

Legacy configs are migrated automatically, preserving filter values, bypass flags,
and the original order. An explicit `filters = []` means no filters, not defaults.
Older configs without limiter/profile settings load with the limiter disabled
and no saved profiles. New saves use the ordered format. For example:

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

The UI publishes filters and limiter together as complete chain snapshots. DSP uses a non-blocking mutex read at
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
    profiles.rs    # named profile management and TOML exchange
    tuning.rs      # dry recording and headphone replay controls
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
    profiles.rs    # profile validation, snapshots, and versioned TOML exchange
    tests.rs       # serialization and migration regression tests
  audio/
    devices.rs     # enumeration and device routing policy
    engine.rs      # stream lifecycle and worker ownership
    streams.rs     # format/config selection and callback construction
    samples.rs     # allocation-free PCM conversion and ring I/O
    processing.rs  # DSP worker, output sinks, buffered resampling
    meters.rs      # shared peak meters
    tuning.rs      # bounded in-memory recording and monitor-only looping
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
