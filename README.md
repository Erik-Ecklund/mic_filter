# Mic Processor (`mic_filter`)

Real-time microphone conditioner for Windows. Captures your physical mic, runs a fixed DSP chain, and writes the result to a virtual cable so Discord / Zoom / browsers can use the cleaned signal as their microphone.

## Processing chain

1. **High-pass** (~80 Hz Butterworth) — optional rumble filter  
2. **RNNoise** (`nnnoiseless`) — neural noise suppression at 48 kHz / 480-sample frames  
3. **Downward expander** — quiets residual noise in pauses  
4. **Soft-knee compressor** — evening out speech dynamics  
5. **Master gain**

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

A settings window and system-tray icon appear. Tray actions: Open settings, Start/Stop, Toggle mute, Quit.

## Configuration

Persisted to:

`%APPDATA%\mic-processor\mic-processor\config.toml`

(via the `directories` crate project path)

| Setting | Meaning |
|--------|---------|
| `input_device` / `output_device` | Friendly device names |
| `highpass_*` | Pre-RNNoise HPF |
| `rnnoise_enabled` | Toggle noise suppression |
| `expander.*` | threshold, ratio, attack/release/hold, range |
| `compressor.*` | threshold, ratio, attack/release, knee, makeup |
| `master_gain_db` | Final output gain |
| `start_processing` | Auto-start engine on launch |

Live parameter changes apply immediately (atomics). Device changes need **Apply devices (restart)**.

## Latency & performance

- Target path: shared-mode WASAPI via cpal, 10 ms RNNoise frames, small ring priming (~20 ms).
- Expect roughly **30–50 ms** perceived latency depending on device buffer sizes.
- Underrun/overrun counters are shown in the UI for debugging.

## Project layout

```
src/
  main.rs          # tray + egui event loop
  ui.rs            # settings UI
  config.rs        # TOML config + LiveParams
  audio/           # devices, engine, meters
  dsp/             # HPF, RNNoise, expander, compressor, chain
plan.md
```

## Non-goals (v1)

- Acoustic echo cancellation
- VST hosting
- Cross-platform polish (Windows-first; cpal may work elsewhere but is untested)
- Automatic VB-Cable install

## License

MIT
