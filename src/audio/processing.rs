use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use ringbuf::traits::{Consumer, Producer};
use rubato::{FftFixedInOut, Resampler};

use super::{PROCESS_CHUNK, PROCESS_RATE, engine::EngineStatus, meters::LevelMeters};
use crate::config::LiveParams;
use crate::dsp::DspChain;

pub(super) struct ProcessingRates {
    pub input: u32,
    pub output: u32,
    pub monitor: u32,
}

pub(super) struct ProcessingContext {
    pub params: Arc<LiveParams>,
    pub meters: Arc<LevelMeters>,
    pub status: Arc<EngineStatus>,
    pub stop: Arc<AtomicBool>,
    pub rates: ProcessingRates,
}

/// Resampling state and scratch buffers have one owner and are reused per block.
struct BlockResampler {
    resampler: FftFixedInOut<f32>,
    input: Vec<Vec<f32>>,
    output: Vec<Vec<f32>>,
}

impl BlockResampler {
    fn new(from: u32, to: u32) -> Result<Self> {
        let resampler = FftFixedInOut::new(from as usize, to as usize, PROCESS_CHUNK, 1)?;
        let input = vec![vec![0.0; resampler.input_frames_max()]];
        let output = vec![vec![0.0; resampler.output_frames_max()]];
        Ok(Self {
            resampler,
            input,
            output,
        })
    }

    fn input_frames_next(&self) -> usize {
        self.resampler.input_frames_next()
    }

    fn process(&mut self, samples: &[f32]) -> Result<&[f32]> {
        let need = self.input_frames_next();
        if samples.len() != need {
            tracing::warn!(need, got = samples.len(), "resampler size mismatch");
        }
        if self.input[0].len() < samples.len() {
            self.input[0].resize(samples.len(), 0.0);
        }
        self.input[0][..samples.len()].copy_from_slice(samples);
        let (_, produced) =
            self.resampler
                .process_into_buffer(&self.input, &mut self.output, None)?;
        Ok(&self.output[0][..produced])
    }
}

fn optional_resampler(
    from: u32,
    to: u32,
    label: &'static str,
    status: &EngineStatus,
) -> Option<BlockResampler> {
    if from == to || to == 0 {
        return None;
    }
    match BlockResampler::new(from, to) {
        Ok(resampler) => Some(resampler),
        Err(error) => {
            tracing::error!(%error, "{label} resampler init failed");
            if label != "monitor" {
                *status.last_error.lock() = Some(format!("{label} resampler: {error}"));
            }
            None
        }
    }
}

/// Shared delivery for cable and monitor. Only the cable counts dropped blocks,
/// matching the original overrun policy; monitoring is best-effort.
struct OutputSink<P> {
    producer: P,
    resampler: Option<BlockResampler>,
    count_overruns: bool,
}

impl<P: Producer<Item = f32>> OutputSink<P> {
    fn write(&mut self, samples: &[f32], status: &EngineStatus) {
        let samples = if let Some(resampler) = &mut self.resampler {
            match resampler.process(samples) {
                Ok(samples) => samples,
                Err(_) => return,
            }
        } else {
            samples
        };
        for &sample in samples {
            if self.producer.try_push(sample).is_err() && self.count_overruns {
                status.overruns.fetch_add(1, Ordering::Relaxed);
                break;
            }
        }
    }
}

pub(super) fn process_loop(
    mut capture: impl Consumer<Item = f32> + Send + 'static,
    playback: impl Producer<Item = f32> + Send + 'static,
    monitor: Option<impl Producer<Item = f32> + Send + 'static>,
    context: ProcessingContext,
) {
    let ProcessingContext {
        params,
        meters,
        status,
        stop,
        rates,
    } = context;
    let mut chain = DspChain::new(params.clone());
    let mut replay_chain = None;
    let mut input_resampler = optional_resampler(rates.input, PROCESS_RATE, "input", &status);
    let mut output = OutputSink {
        producer: playback,
        resampler: optional_resampler(PROCESS_RATE, rates.output, "output", &status),
        count_overruns: true,
    };
    let monitor_resampler = optional_resampler(PROCESS_RATE, rates.monitor, "monitor", &status);
    let mut monitor = monitor.map(|producer| OutputSink {
        producer,
        resampler: monitor_resampler,
        count_overruns: false,
    });
    let mut input_buffer = [0.0; PROCESS_CHUNK * 4];
    let mut captured = Vec::with_capacity(PROCESS_CHUNK * 4);
    let mut resampled = Vec::with_capacity(PROCESS_CHUNK * 4);
    let mut work = [0.0; PROCESS_CHUNK];
    let mut monitor_work = [0.0; PROCESS_CHUNK];

    while !stop.load(Ordering::SeqCst) {
        let available = capture.occupied_len();
        if available == 0 {
            thread::sleep(Duration::from_millis(1));
            continue;
        }
        let to_read = available.min(input_buffer.len());
        let n = capture.pop_slice(&mut input_buffer[..to_read]);
        captured.extend_from_slice(&input_buffer[..n]);

        let pending = if let Some(resampler) = &mut input_resampler {
            let mut need = resampler.input_frames_next();
            while need > 0 && captured.len() >= need {
                if let Ok(produced) = resampler.process(&captured[..need]) {
                    meters.push_input(produced);
                    resampled.extend_from_slice(produced);
                }
                captured.drain(..need);
                need = resampler.input_frames_next();
            }
            &mut resampled
        } else {
            meters.push_input(&captured);
            &mut captured
        };

        while pending.len() >= PROCESS_CHUNK {
            work.copy_from_slice(&pending[..PROCESS_CHUNK]);
            pending.drain(..PROCESS_CHUNK);
            let dry = work;
            chain.process(&mut work);
            meters.push_output(&work);
            output.write(&work, &status);
            params
                .tuning
                .process(&dry, &work, &mut monitor_work, |samples, reset| {
                    if reset || replay_chain.is_none() {
                        replay_chain = Some(DspChain::new(params.clone()));
                    }
                    if let Some(chain) = &mut replay_chain {
                        chain.process(samples);
                    }
                });
            // Original comparison bypasses the chain, but never global mute.
            if params.muted.load(Ordering::Relaxed) {
                monitor_work.fill(0.0);
            }
            if let Some(monitor) = &mut monitor {
                monitor.write(&monitor_work, &status);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ringbuf::{
        HeapRb,
        traits::{Consumer, Observer, Split},
    };

    #[test]
    fn buffered_resampling_matches_rubato_and_reuses_storage() {
        for (from, to) in [(44_100, 48_000), (48_000, 44_100), (48_000, 96_000)] {
            let mut buffered = BlockResampler::new(from, to).unwrap();
            let input_ptr = buffered.input[0].as_ptr();
            let output_ptr = buffered.output[0].as_ptr();
            let mut reference =
                FftFixedInOut::<f32>::new(from as usize, to as usize, PROCESS_CHUNK, 1).unwrap();
            for frame in 0..4 {
                let n = reference.input_frames_next();
                let input: Vec<_> = (0..n)
                    .map(|i| ((i + frame * n) as f32 * 0.03).sin() * 0.25)
                    .collect();
                let expected = reference
                    .process(std::slice::from_ref(&input), None)
                    .unwrap();
                let actual = buffered.process(&input).unwrap();
                assert_eq!(actual, expected[0]);
                assert!(actual.iter().all(|sample| sample.is_finite()));
                assert_eq!(buffered.input[0].as_ptr(), input_ptr);
                assert_eq!(buffered.output[0].as_ptr(), output_ptr);
            }
        }
    }

    #[test]
    fn output_sinks_keep_cable_and_monitor_overrun_policies() {
        let status = EngineStatus::new();
        let (producer, mut consumer) = HeapRb::<f32>::new(2).split();
        let mut cable = OutputSink {
            producer,
            resampler: None,
            count_overruns: true,
        };
        cable.write(&[0.1, 0.2, 0.3, 0.4], &status);
        assert_eq!(status.overruns.load(Ordering::Relaxed), 1);
        assert_eq!(consumer.try_pop(), Some(0.1));
        assert_eq!(consumer.try_pop(), Some(0.2));
        let (producer, mut consumer) = HeapRb::<f32>::new(2).split();
        let mut monitor = OutputSink {
            producer,
            resampler: None,
            count_overruns: false,
        };
        monitor.write(&[0.3, 0.4, 0.5], &status);
        assert_eq!(status.overruns.load(Ordering::Relaxed), 1);
        assert_eq!(consumer.try_pop(), Some(0.3));
        assert_eq!(consumer.try_pop(), Some(0.4));
    }

    #[test]
    fn equal_rates_and_absent_monitor_do_not_create_resamplers() {
        let status = EngineStatus::new();
        assert!(optional_resampler(PROCESS_RATE, PROCESS_RATE, "output", &status).is_none());
        assert!(optional_resampler(PROCESS_RATE, 0, "monitor", &status).is_none());
    }

    #[test]
    fn replay_stays_on_monitor_tracks_edits_and_respects_mute() {
        use crate::audio::Comparison;
        use crate::config::{AppConfig, FilterConfig, FilterSettings, GainConfig};
        use std::time::Instant;

        let params = LiveParams::from_config(&AppConfig {
            filters: Vec::new(),
            ..AppConfig::default()
        });
        let (mut capture, capture_consumer) = HeapRb::<f32>::new(PROCESS_CHUNK * 4).split();
        let (playback, mut cable) = HeapRb::<f32>::new(PROCESS_CHUNK * 4).split();
        let (monitor, mut headphones) = HeapRb::<f32>::new(PROCESS_CHUNK * 4).split();
        let stop = Arc::new(AtomicBool::new(false));
        let context = ProcessingContext {
            params: params.clone(),
            meters: LevelMeters::new(),
            status: EngineStatus::new(),
            stop: stop.clone(),
            rates: ProcessingRates {
                input: PROCESS_RATE,
                output: PROCESS_RATE,
                monitor: PROCESS_RATE,
            },
        };
        let worker = thread::spawn(move || {
            process_loop(capture_consumer, playback, Some(monitor), context);
        });
        let mut exchange = |input: f32| {
            assert_eq!(capture.push_slice(&[input; PROCESS_CHUNK]), PROCESS_CHUNK);
            let deadline = Instant::now() + Duration::from_secs(5);
            while (cable.occupied_len() < PROCESS_CHUNK
                || headphones.occupied_len() < PROCESS_CHUNK)
                && Instant::now() < deadline
            {
                thread::sleep(Duration::from_millis(1));
            }
            let mut output = [0.0; PROCESS_CHUNK];
            let mut monitor = [0.0; PROCESS_CHUNK];
            assert_eq!(cable.pop_slice(&mut output), PROCESS_CHUNK);
            assert_eq!(headphones.pop_slice(&mut monitor), PROCESS_CHUNK);
            (output, monitor)
        };
        params.tuning.record();
        params.muted.store(true, Ordering::Relaxed);
        assert_eq!(
            exchange(0.125),
            ([0.0; PROCESS_CHUNK], [0.0; PROCESS_CHUNK])
        );
        params.tuning.stop();
        params.muted.store(false, Ordering::Relaxed);
        params.tuning.set_comparison(Comparison::Processed, false);
        params.tuning.play();
        assert_eq!(
            exchange(0.5),
            ([0.5; PROCESS_CHUNK], [0.125; PROCESS_CHUNK])
        );
        params.publish_filters(&[FilterConfig::new(
            1,
            FilterSettings::Gain(GainConfig {
                enabled: true,
                gain_db: -6.020_6,
            }),
        )]);
        let (cable, monitor) = exchange(0.5);
        assert!(cable.iter().all(|s| (*s - 0.25).abs() < 1e-6));
        assert!(monitor.iter().all(|s| (*s - 0.0625).abs() < 1e-6));
        params.tuning.set_comparison(Comparison::Original, false);
        let (_, monitor) = exchange(0.5);
        assert_eq!(monitor, [0.125; PROCESS_CHUNK]);
        params.muted.store(true, Ordering::Relaxed);
        assert_eq!(exchange(0.5), ([0.0; PROCESS_CHUNK], [0.0; PROCESS_CHUNK]));
        params.muted.store(false, Ordering::Relaxed);
        params.tuning.stop();
        let (cable, monitor) = exchange(0.5);
        assert_eq!(cable, monitor);
        assert!(monitor.iter().all(|s| (*s - 0.25).abs() < 1e-6));
        stop.store(true, Ordering::SeqCst);
        worker.join().unwrap();
    }

    #[test]
    fn worker_matches_reference_pipeline_and_feeds_disabled_monitor() {
        use super::super::RING_FRAMES;
        use crate::config::AppConfig;
        use std::time::Instant;

        for (input_rate, output_rate) in [
            (48_000, 48_000),
            (44_100, 48_000),
            (48_000, 44_100),
            (48_000, 96_000),
        ] {
            let mut input_resampler = (input_rate != PROCESS_RATE).then(|| {
                FftFixedInOut::<f32>::new(
                    input_rate as usize,
                    PROCESS_RATE as usize,
                    PROCESS_CHUNK,
                    1,
                )
                .unwrap()
            });
            let input_frames = input_resampler
                .as_ref()
                .map_or(PROCESS_CHUNK, |resampler| resampler.input_frames_next());
            let input: Vec<_> = (0..input_frames * 4)
                .map(|i| (i as f32 * 0.03).sin() * 0.25)
                .collect();
            let mut mono = Vec::new();
            if let Some(resampler) = &mut input_resampler {
                for chunk in input.chunks_exact(input_frames) {
                    mono.extend_from_slice(&resampler.process(&[chunk], None).unwrap()[0]);
                }
            } else {
                mono.extend_from_slice(&input);
            }
            let mut output_resampler = (output_rate != PROCESS_RATE).then(|| {
                FftFixedInOut::<f32>::new(
                    PROCESS_RATE as usize,
                    output_rate as usize,
                    PROCESS_CHUNK,
                    1,
                )
                .unwrap()
            });
            let mut expected = Vec::new();
            for chunk in mono
                .chunks(PROCESS_CHUNK)
                .filter(|chunk| chunk.len() == PROCESS_CHUNK)
            {
                if let Some(resampler) = &mut output_resampler {
                    expected.extend_from_slice(&resampler.process(&[chunk], None).unwrap()[0]);
                } else {
                    expected.extend_from_slice(chunk);
                }
            }

            let (mut capture, capture_consumer) = HeapRb::<f32>::new(RING_FRAMES).split();
            assert_eq!(capture.push_slice(&input), input.len());
            let (playback, mut playback_consumer) = HeapRb::<f32>::new(RING_FRAMES).split();
            let (monitor, mut monitor_consumer) = HeapRb::<f32>::new(RING_FRAMES).split();
            let stop = Arc::new(AtomicBool::new(false));
            let status = EngineStatus::new();
            let context = ProcessingContext {
                params: LiveParams::from_config(&AppConfig {
                    filters: Vec::new(),
                    ..AppConfig::default()
                }),
                meters: LevelMeters::new(),
                status: status.clone(),
                stop: stop.clone(),
                rates: ProcessingRates {
                    input: input_rate,
                    output: output_rate,
                    monitor: output_rate,
                },
            };
            let worker = thread::spawn(move || {
                process_loop(capture_consumer, playback, Some(monitor), context);
            });
            let deadline = Instant::now() + Duration::from_secs(5);
            while (playback_consumer.occupied_len() < expected.len()
                || monitor_consumer.occupied_len() < expected.len())
                && Instant::now() < deadline
            {
                thread::sleep(Duration::from_millis(1));
            }
            stop.store(true, Ordering::SeqCst);
            worker.join().unwrap();
            let mut actual = vec![0.0; expected.len()];
            assert_eq!(playback_consumer.pop_slice(&mut actual), expected.len());
            assert_eq!(actual, expected, "cable {input_rate} -> {output_rate}");
            assert_eq!(monitor_consumer.pop_slice(&mut actual), expected.len());
            assert_eq!(actual, expected, "monitor {input_rate} -> {output_rate}");
            assert_eq!(status.overruns.load(Ordering::Relaxed), 0);
        }
    }
}
