# Real-Time Microphone Processor (Rust)

## Goal

Replace the OBS + VB-Audio Virtual Cable monitoring setup with a lightweight Rust background application that:

1. Captures audio from a physical microphone via WASAPI
2. Applies a fixed filter chain in order: **Noise Suppression (RNNoise) → Expander → Compressor**
3. Writes the processed audio to a virtual cable input (VB-Cable “CABLE Input”)
4. Allows the virtual cable’s output (“CABLE Output”) to be selected as the Windows default communication device

The application should be low-latency, low-CPU, and run as a system tray utility with a simple GUI for device selection and filter parameters.

## High-Level Architecture

```
Physical Mic
    ↓
WASAPI Capture (shared, event-driven, low-latency)
    ↓
Ring buffer (lock-free)
    ↓
Processing thread (48 kHz mono float)
    ├── High-pass (optional, ~80 Hz)
    ├── RNNoise (noise suppression)
    ├── Expander (downward expander)
    └── Compressor (soft-knee)
    ↓
Ring buffer
    ↓
WASAPI Render → “CABLE Input” (VB-Cable)
    ↓
“CABLE Output” (selected by other apps)
```

Optional local monitoring path (render to headphones) can be added later.

## Recommended Crates

| Purpose              | Crate                          | Notes |
|----------------------|--------------------------------|-------|
| Audio I/O            | `wasapi` or `cpal`             | Prefer `wasapi` crate for fine-grained control and low latency |
| RNNoise              | `nnnoiseless`                  | Pure-Rust port of RNNoise; works on 480-sample frames at 48 kHz |
| Ring buffers         | `ringbuf`                      | Lock-free SPSC |
| GUI / tray           | `tray-icon` + `winit` + `egui` or `slint` | Lightweight tray app |
| Config persistence   | `serde` + `toml` or `serde_json` | |
| Logging              | `tracing` + `tracing-subscriber` | |
| Error handling       | `anyhow` + `thiserror`         | |
| Device enumeration   | `wasapi` or `windows-sys`      | |

Alternative: use `cpal` for simpler cross-platform code if pure Windows is not required.

## Project Structure

```
mic-processor/
├── Cargo.toml
├── README.md
├── src/
│   ├── main.rs                 # Entry point, tray, event loop
│   ├── audio/
│   │   ├── mod.rs
│   │   ├── capture.rs          # WASAPI capture setup & callback
│   │   ├── render.rs           # WASAPI render to virtual cable
│   │   └── devices.rs          # Device enumeration & selection
│   ├── dsp/
│   │   ├── mod.rs
│   │   ├── noise_suppression.rs # RNNoise wrapper (nnnoiseless)
│   │   ├── expander.rs         # Downward expander / soft gate
│   │   ├── compressor.rs       # Soft-knee compressor
│   │   └── chain.rs            # Ordered processing chain
│   ├── config.rs               # Load/save settings
│   └── ui.rs                   # Tray menu + optional settings window
└── plan.md                     # This file
```

## Detailed Implementation Plan

### 1. Audio I/O Layer

- Use WASAPI shared mode, event-driven, with a small buffer period (aim for 10–20 ms total latency).
- Capture from the selected physical microphone.
- Convert captured audio to mono `f32` at 48 kHz (RNNoise native rate).
- Use lock-free ring buffers (`ringbuf`) between the WASAPI callback threads and the processing thread.
- Render processed audio to the selected output device (normally “CABLE Input”).
- Handle device hot-plug / default device changes gracefully.
- Expose device lists so the UI can let the user pick input and output.

### 2. Processing Chain (DSP)

Process in this exact order on 48 kHz mono float frames (preferably 480-sample blocks to match RNNoise):

1. **Optional high-pass filter** (~80 Hz Butterworth) to remove rumble before RNNoise.
2. **Noise Suppression** using `nnnoiseless::DenoiseState`.
   - Feed exactly 480 samples at a time.
   - Handle partial frames with internal buffering.
3. **Expander** (downward expander):
   - Parameters: threshold (dB), ratio, attack (ms), release (ms), hold (ms), range (dB).
   - Classic RMS or peak detector with hysteresis or simple envelope follower.
   - Goal: further reduce residual noise in pauses after RNNoise.
4. **Compressor**:
   - Parameters: threshold (dB), ratio, attack (ms), release (ms), knee (dB), makeup gain (dB).
   - Soft-knee feed-forward implementation.
   - Optional auto-makeup.

All DSP must be allocation-free and lock-free in the processing path. Parameter changes from the UI should be applied safely (atomic floats or a small lock-free parameter queue).

### 3. Configuration & UI

- Persist settings to `%APPDATA%/mic-processor/config.toml` (or similar).
- Settings to expose:
  - Selected input device (by ID or friendly name)
  - Selected output device (default to VB-Cable if detected)
  - RNNoise strength / VAD threshold (if exposed by the crate)
  - Expander: threshold, ratio, attack, release, hold
  - Compressor: threshold, ratio, attack, release, knee, makeup
  - Master output gain
  - Start with Windows / start minimized
- System tray icon with:
  - Start / Stop processing
  - Quick mute
  - Open settings
  - Quit
- Optional simple settings window (egui or slint) for parameter adjustment and live meters (input/output level).

### 4. Latency & Real-Time Constraints

- Target end-to-end latency ≤ 30–40 ms.
- Use MMCSS “Pro Audio” or “Games” thread priority on the processing and audio threads where possible.
- Avoid any blocking calls, logging, or allocations inside the audio/processing hot path.
- Measure and log buffer underruns / overruns for debugging.

### 5. VB-Cable Integration

- Detect common VB-Cable device names (“CABLE Input”, “CABLE Output”, “VB-Audio Virtual Cable”).
- On first run, suggest or auto-select the cable if present.
- Document that the user must:
  1. Install VB-Cable (https://vb-audio.com/Cable/)
  2. Set “CABLE Output” as the default communication device (or select it per-app)
  3. Run this program with output directed at “CABLE Input”

### 6. Error Handling & Robustness

- Graceful handling of device removal, format changes, and exclusive-mode conflicts.
- Clear error messages in the tray tooltip or a small notification.
- Restart processing automatically if a transient error occurs.
- Log to a file for debugging (`tracing`).

## Implementation Order for the Agent

1. **Scaffold** the Cargo project with the crates listed above.
2. **Device enumeration** and basic WASAPI capture → file or speakers (sanity check).
3. **Implement the ring-buffer pipeline** and a passthrough processing thread.
4. **Integrate `nnnoiseless`** and verify noise suppression works.
5. **Implement expander and compressor** (start with simple versions; refine later).
6. **Wire the full chain** and test with VB-Cable.
7. **Add configuration persistence**.
8. **Add system tray + basic controls**.
9. **Add optional settings UI** with live meters.
10. **Polish**: latency measurement, underrun counters, auto-start, installer notes.

## Testing Checklist

- [ ] Captures from physical mic without dropouts
- [ ] RNNoise reduces background noise while preserving voice
- [ ] Expander further quiets pauses
- [ ] Compressor evens out loud/quiet speech
- [ ] Processed audio appears cleanly on “CABLE Output”
- [ ] Discord / Zoom / browser can use “CABLE Output” as mic
- [ ] Latency feels acceptable for conversation (< 50 ms perceived)
- [ ] Device unplug/replug is handled
- [ ] Settings survive restart
- [ ] CPU usage stays low (< 5 % on a modern CPU)

## Non-Goals (for v1)

- Acoustic echo cancellation (AEC)
- Full VST hosting
- Custom kernel-mode virtual driver
- Cross-platform support (Windows-only is fine)
- Automatic VB-Cable installation

## References

- RNNoise: https://github.com/xiph/rnnoise
- nnnoiseless: https://crates.io/crates/nnnoiseless
- Example projects for inspiration: NoiseGate, Getsu, Easy MIC Booster, mic-booster
- WASAPI low-latency patterns: Microsoft docs + `wasapi` crate examples

## Deliverables

- Working Rust application (system tray)
- README with build instructions, VB-Cable setup steps, and parameter explanations
- Basic config file format
- Source that a future agent or human can extend (e.g., add EQ, limiter, AEC)
