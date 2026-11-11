use parking_lot::Mutex;

use super::{PROCESS_CHUNK, PROCESS_RATE};

pub const MAX_RECORD_SECONDS: usize = 30;
const MAX_SAMPLES: usize = PROCESS_RATE as usize * MAX_RECORD_SECONDS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Comparison {
    Processed,
    Original,
}

#[derive(Clone, Copy, Debug)]
pub struct TuningSnapshot {
    pub recording: bool,
    pub playing: bool,
    pub seconds: f32,
    pub position_seconds: f32,
    pub comparison: Comparison,
    pub match_loudness: bool,
}

struct State {
    clip: Vec<f32>,
    recording: bool,
    playing: bool,
    position: usize,
    reset_replay: bool,
    comparison: Comparison,
    match_loudness: bool,
    dry_power: f32,
    wet_power: f32,
    measured: bool,
}

/// Only the UI and DSP worker touch this state, never device callbacks.
/// The worker skips a contended block rather than waiting for UI actions.
pub struct Tuning {
    state: Mutex<State>,
}

impl Tuning {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State {
                clip: Vec::new(),
                recording: false,
                playing: false,
                position: 0,
                reset_replay: true,
                comparison: Comparison::Processed,
                match_loudness: true,
                dry_power: 0.0,
                wet_power: 0.0,
                measured: false,
            }),
        }
    }

    pub fn snapshot(&self) -> TuningSnapshot {
        let state = self.state.lock();
        TuningSnapshot {
            recording: state.recording,
            playing: state.playing,
            seconds: state.clip.len() as f32 / PROCESS_RATE as f32,
            position_seconds: state.position as f32 / PROCESS_RATE as f32,
            comparison: state.comparison,
            match_loudness: state.match_loudness,
        }
    }

    pub fn record(&self) {
        // Allocate on the UI thread, not while capturing audio.
        let clip = Vec::with_capacity(MAX_SAMPLES);
        let mut state = self.state.lock();
        state.clip = clip;
        state.recording = true;
        state.playing = false;
        state.position = 0;
    }

    pub fn play(&self) {
        let mut state = self.state.lock();
        if state.clip.is_empty() || state.recording {
            return;
        }
        state.playing = true;
        state.position = 0;
        state.reset_replay = true;
        state.measured = false;
    }

    pub fn stop(&self) {
        let mut state = self.state.lock();
        state.recording = false;
        state.playing = false;
        state.position = 0;
    }

    pub fn clear(&self) {
        let mut state = self.state.lock();
        state.recording = false;
        state.playing = false;
        state.position = 0;
        state.clip.clear();
    }

    pub fn set_comparison(&self, comparison: Comparison, match_loudness: bool) {
        let mut state = self.state.lock();
        state.comparison = comparison;
        state.match_loudness = match_loudness;
    }

    /// Capture before DSP, then select audio for the monitor only.
    /// Replay always runs through its own chain, even when listening to Original.
    pub(super) fn process(
        &self,
        dry: &[f32; PROCESS_CHUNK],
        live: &[f32; PROCESS_CHUNK],
        monitor: &mut [f32; PROCESS_CHUNK],
        mut process_replay: impl FnMut(&mut [f32], bool),
    ) {
        let Some(mut state) = self.state.try_lock() else {
            monitor.fill(0.0);
            return;
        };
        if state.recording {
            state.clip.extend_from_slice(dry);
            if state.clip.len() >= MAX_SAMPLES {
                state.recording = false;
            }
        }
        if !state.playing {
            monitor.copy_from_slice(live);
            return;
        }
        // Clips contain whole 10 ms blocks; length is always a multiple of 480.
        let mut original = [0.0; PROCESS_CHUNK];
        original.copy_from_slice(&state.clip[state.position..state.position + PROCESS_CHUNK]);
        state.position = (state.position + PROCESS_CHUNK) % state.clip.len();
        monitor.copy_from_slice(&original);
        process_replay(monitor, state.reset_replay);
        state.reset_replay = false;

        let power =
            |samples: &[f32]| samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32;
        // Approximately 500 ms RMS smoothing. Attenuate only: never amplify
        // suppressed noise or quiet pauses to force a loudness match.
        let alpha = if state.measured { 0.02 } else { 1.0 };
        state.dry_power += alpha * (power(&original) - state.dry_power);
        state.wet_power += alpha * (power(monitor) - state.wet_power);
        state.measured = true;
        let target = state.dry_power.min(state.wet_power);
        let selected_power = if state.comparison == Comparison::Original {
            monitor.copy_from_slice(&original);
            state.dry_power
        } else {
            state.wet_power
        };
        if state.match_loudness && selected_power > 1e-12 {
            let gain = (target / selected_power).clamp(0.0, 1.0).sqrt();
            for sample in monitor {
                *sample *= gain;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_stop_retains_clip_but_cancels_recording_and_replay() {
        use crate::audio::{AudioEngine, LevelMeters};
        use crate::config::{AppConfig, LiveParams};

        let params = LiveParams::from_config(&AppConfig::default());
        let engine = AudioEngine::new(params.clone(), LevelMeters::new());
        params.tuning.record();
        block(&params.tuning, 0.25, 0.0, 1.0);
        engine.stop();
        assert!(!params.tuning.snapshot().recording);
        assert_eq!(params.tuning.snapshot().seconds, 0.01);
        params.tuning.play();
        assert!(params.tuning.snapshot().playing);
        engine.stop();
        assert!(!params.tuning.snapshot().playing);
        assert_eq!(params.tuning.snapshot().seconds, 0.01);
    }

    fn block(tuning: &Tuning, dry: f32, live: f32, gain: f32) -> [f32; PROCESS_CHUNK] {
        let mut monitor = [0.0; PROCESS_CHUNK];
        tuning.process(
            &[dry; PROCESS_CHUNK],
            &[live; PROCESS_CHUNK],
            &mut monitor,
            |samples, _| {
                for sample in samples {
                    *sample *= gain;
                }
            },
        );
        monitor
    }

    #[test]
    fn records_dry_audio_loops_and_never_changes_live_input() {
        let tuning = Tuning::new();
        tuning.record();
        assert_eq!(block(&tuning, 0.2, 0.8, 1.0), [0.8; PROCESS_CHUNK]);
        block(&tuning, 0.4, 0.8, 1.0);
        tuning.stop();
        tuning.set_comparison(Comparison::Processed, false);
        tuning.play();
        for expected in [0.4, 0.8, 0.4, 0.8] {
            assert_eq!(block(&tuning, 0.9, 0.7, 2.0), [expected; PROCESS_CHUNK]);
        }
        tuning.set_comparison(Comparison::Original, false);
        assert_eq!(block(&tuning, 0.9, 0.7, 2.0), [0.2; PROCESS_CHUNK]);
        tuning.stop();
        assert_eq!(block(&tuning, 0.9, 0.7, 2.0), [0.7; PROCESS_CHUNK]);
        assert_eq!(tuning.snapshot().seconds, 0.02);
    }

    #[test]
    fn recording_is_bounded_and_new_recording_replaces_clip() {
        let tuning = Tuning::new();
        tuning.play();
        assert!(!tuning.snapshot().playing);
        tuning.record();
        tuning.play();
        assert!(!tuning.snapshot().playing);
        for _ in 0..MAX_SAMPLES / PROCESS_CHUNK + 10 {
            block(&tuning, 0.1, 0.0, 1.0);
        }
        assert!(!tuning.snapshot().recording);
        assert_eq!(tuning.state.lock().clip.len(), MAX_SAMPLES);
        tuning.record();
        assert_eq!(tuning.snapshot().seconds, 0.0);
        block(&tuning, 0.3, 0.0, 1.0);
        tuning.stop();
        tuning.play();
        tuning.clear();
        assert!(!tuning.snapshot().playing);
        assert_eq!(tuning.snapshot().seconds, 0.0);
    }

    #[test]
    fn comparison_matches_rms_without_boosting_and_silence_is_finite() {
        let tuning = Tuning::new();
        tuning.record();
        block(&tuning, 0.25, 0.0, 1.0);
        tuning.stop();
        tuning.play();
        let processed = block(&tuning, 0.0, 0.0, 2.0);
        assert!(processed.iter().all(|s| (*s - 0.25).abs() < 1e-6));
        tuning.set_comparison(Comparison::Original, true);
        let original = block(&tuning, 0.0, 0.0, 2.0);
        assert!(original.iter().all(|s| (*s - 0.25).abs() < 1e-6));
        tuning.record();
        block(&tuning, 0.0, 0.0, 1.0);
        tuning.stop();
        tuning.play();
        assert_eq!(block(&tuning, 0.0, 0.0, 1.0), [0.0; PROCESS_CHUNK]);
    }

    #[test]
    fn contended_ui_does_not_block_worker_or_leak_live_audio() {
        let tuning = Tuning::new();
        let _guard = tuning.state.lock();
        assert_eq!(block(&tuning, 0.5, 0.8, 1.0), [0.0; PROCESS_CHUNK]);
    }

    #[test]
    fn matching_attenuates_original_when_processing_is_quieter() {
        let tuning = Tuning::new();
        tuning.record();
        block(&tuning, 0.5, 0.0, 1.0);
        tuning.stop();
        tuning.play();
        let processed = block(&tuning, 0.0, 0.0, 0.25);
        assert_eq!(processed, [0.125; PROCESS_CHUNK]);
        tuning.set_comparison(Comparison::Original, true);
        let original = block(&tuning, 0.0, 0.0, 0.25);
        assert!(original.iter().all(|s| (*s - 0.125).abs() < 1e-6));
    }

    #[test]
    fn replay_resets_only_on_start_and_keeps_processing_during_original() {
        let tuning = Tuning::new();
        tuning.record();
        block(&tuning, 0.25, 0.0, 1.0);
        tuning.stop();
        tuning.set_comparison(Comparison::Original, false);
        tuning.play();
        for expected_reset in [true, false, false] {
            tuning.process(
                &[0.0; PROCESS_CHUNK],
                &[0.0; PROCESS_CHUNK],
                &mut [0.0; PROCESS_CHUNK],
                |_, reset| assert_eq!(reset, expected_reset),
            );
        }
    }
}
