use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use parking_lot::Mutex;
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::HeapRb;
use rubato::{FftFixedInOut, Resampler};

use super::devices::{find_input_device, find_output_device};
use super::meters::LevelMeters;
use crate::config::LiveParams;
use crate::dsp::DspChain;

pub const PROCESS_RATE: u32 = 48_000;
const PROCESS_CHUNK: usize = 480; // RNNoise frame / 10 ms
const RING_FRAMES: usize = PROCESS_RATE as usize / 2; // ~500 ms capacity

pub struct EngineStatus {
    pub underruns: AtomicU64,
    pub overruns: AtomicU64,
    pub last_error: Mutex<Option<String>>,
    pub input_name: Mutex<String>,
    pub output_name: Mutex<String>,
    pub input_rate: AtomicU64,
    pub output_rate: AtomicU64,
}

impl EngineStatus {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            underruns: AtomicU64::new(0),
            overruns: AtomicU64::new(0),
            last_error: Mutex::new(None),
            input_name: Mutex::new(String::new()),
            output_name: Mutex::new(String::new()),
            input_rate: AtomicU64::new(0),
            output_rate: AtomicU64::new(0),
        })
    }
}

struct RunningStreams {
    _input: Stream,
    _output: Stream,
    stop: Arc<AtomicBool>,
    proc_handle: Option<JoinHandle<()>>,
}

pub struct AudioEngine {
    params: Arc<LiveParams>,
    meters: Arc<LevelMeters>,
    status: Arc<EngineStatus>,
    running: Mutex<Option<RunningStreams>>,
}

impl AudioEngine {
    pub fn new(params: Arc<LiveParams>, meters: Arc<LevelMeters>) -> Self {
        Self {
            params,
            meters,
            status: EngineStatus::new(),
            running: Mutex::new(None),
        }
    }

    pub fn status(&self) -> Arc<EngineStatus> {
        self.status.clone()
    }

    pub fn is_running(&self) -> bool {
        self.running.lock().is_some()
    }

    pub fn start(&self, input_name: Option<&str>, output_name: Option<&str>) -> Result<()> {
        self.stop();

        let input_device = find_input_device(input_name)?;
        let output_device = find_output_device(output_name)?;

        let in_name = input_device
            .name()
            .unwrap_or_else(|_| "<unknown input>".into());
        let out_name = output_device
            .name()
            .unwrap_or_else(|_| "<unknown output>".into());
        *self.status.input_name.lock() = in_name.clone();
        *self.status.output_name.lock() = out_name.clone();
        tracing::info!(%in_name, %out_name, "starting audio engine");

        let in_supported = input_device
            .default_input_config()
            .context("input default config")?;
        let out_supported = output_device
            .default_output_config()
            .context("output default config")?;

        let in_sample_format = in_supported.sample_format();
        let out_sample_format = out_supported.sample_format();

        let mut in_config: StreamConfig = in_supported.config();
        let mut out_config: StreamConfig = out_supported.config();

        // Prefer 48 kHz when the device allows it.
        if let Ok(configs) = input_device.supported_input_configs() {
            if let Some(range) = configs
                .filter(|c| c.sample_format() == SampleFormat::F32 || c.sample_format() == in_sample_format)
                .find(|c| c.min_sample_rate().0 <= PROCESS_RATE && c.max_sample_rate().0 >= PROCESS_RATE)
            {
                in_config.sample_rate = cpal::SampleRate(PROCESS_RATE);
                in_config.channels = range.channels().min(2);
                let _ = range; // silence unused if channels already set
            }
        }
        if let Ok(configs) = output_device.supported_output_configs() {
            if let Some(range) = configs
                .filter(|c| c.sample_format() == SampleFormat::F32 || c.sample_format() == out_sample_format)
                .find(|c| c.min_sample_rate().0 <= PROCESS_RATE && c.max_sample_rate().0 >= PROCESS_RATE)
            {
                out_config.sample_rate = cpal::SampleRate(PROCESS_RATE);
                out_config.channels = range.channels().min(2).max(1);
            }
        }

        // Re-read formats after negotiation attempt — keep original supported format.
        let in_rate = in_config.sample_rate.0;
        let out_rate = out_config.sample_rate.0;
        let in_channels = in_config.channels as usize;
        let out_channels = out_config.channels as usize;
        self.status.input_rate.store(in_rate as u64, Ordering::Relaxed);
        self.status
            .output_rate
            .store(out_rate as u64, Ordering::Relaxed);

        tracing::info!(
            in_rate,
            out_rate,
            in_channels,
            out_channels,
            ?in_sample_format,
            ?out_sample_format,
            "stream configuration"
        );

        let (cap_prod, cap_cons) = HeapRb::<f32>::new(RING_FRAMES).split();
        let (mut play_prod, play_cons) = HeapRb::<f32>::new(RING_FRAMES).split();

        // Prime playback ring with a little silence for clock skew.
        let prime = (PROCESS_RATE as usize) / 50; // 20 ms
        for _ in 0..prime {
            let _ = play_prod.try_push(0.0);
        }

        let stop = Arc::new(AtomicBool::new(false));
        let status = self.status.clone();
        let meters = self.meters.clone();
        let params = self.params.clone();

        // Capture producer side
        let mut cap_prod = cap_prod;
        let status_in = status.clone();
        let build_input = |format: SampleFormat| -> Result<Stream> {
            match format {
                SampleFormat::F32 => {
                    let stream = input_device.build_input_stream(
                        &in_config,
                        move |data: &[f32], _| {
                            push_interleaved_mono(data, in_channels, &mut cap_prod, &status_in);
                        },
                        err_fn,
                        None,
                    )?;
                    Ok(stream)
                }
                SampleFormat::I16 => {
                    let stream = input_device.build_input_stream(
                        &in_config,
                        move |data: &[i16], _| {
                            let mut tmp = Vec::with_capacity(data.len());
                            tmp.extend(data.iter().map(|&s| s as f32 / 32768.0));
                            push_interleaved_mono(&tmp, in_channels, &mut cap_prod, &status_in);
                        },
                        err_fn,
                        None,
                    )?;
                    Ok(stream)
                }
                other => Err(anyhow::anyhow!("unsupported input sample format: {other:?}")),
            }
        };

        let mut play_cons = play_cons;
        let status_out = status.clone();
        let build_output = |format: SampleFormat| -> Result<Stream> {
            match format {
                SampleFormat::F32 => {
                    let stream = output_device.build_output_stream(
                        &out_config,
                        move |data: &mut [f32], _| {
                            pop_mono_to_interleaved(
                                data,
                                out_channels,
                                &mut play_cons,
                                &status_out,
                            );
                        },
                        err_fn,
                        None,
                    )?;
                    Ok(stream)
                }
                SampleFormat::I16 => {
                    let stream = output_device.build_output_stream(
                        &out_config,
                        move |data: &mut [i16], _| {
                            let mut fbuf = vec![0.0f32; data.len()];
                            pop_mono_to_interleaved(
                                &mut fbuf,
                                out_channels,
                                &mut play_cons,
                                &status_out,
                            );
                            for (dst, src) in data.iter_mut().zip(fbuf.iter()) {
                                *dst = (src.clamp(-1.0, 1.0) * 32767.0) as i16;
                            }
                        },
                        err_fn,
                        None,
                    )?;
                    Ok(stream)
                }
                other => Err(anyhow::anyhow!(
                    "unsupported output sample format: {other:?}"
                )),
            }
        };

        let input_stream = build_input(in_sample_format).context("build input stream")?;
        let output_stream = build_output(out_sample_format).context("build output stream")?;

        input_stream.play().context("play input")?;
        output_stream.play().context("play output")?;

        let stop_flag = stop.clone();
        let status_proc = status.clone();
        let proc_handle = thread::Builder::new()
            .name("mic-dsp".into())
            .spawn(move || {
                process_loop(
                    cap_cons,
                    play_prod,
                    params,
                    meters,
                    status_proc,
                    stop_flag,
                    in_rate,
                    out_rate,
                );
            })
            .context("spawn processing thread")?;

        self.params.running.store(true, Ordering::Relaxed);
        *self.running.lock() = Some(RunningStreams {
            _input: input_stream,
            _output: output_stream,
            stop,
            proc_handle: Some(proc_handle),
        });
        *self.status.last_error.lock() = None;
        Ok(())
    }

    pub fn stop(&self) {
        let mut guard = self.running.lock();
        if let Some(mut running) = guard.take() {
            running.stop.store(true, Ordering::SeqCst);
            if let Some(handle) = running.proc_handle.take() {
                let _ = handle.join();
            }
            // Streams drop here and stop.
        }
        self.params.running.store(false, Ordering::Relaxed);
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.stop();
    }
}

fn err_fn(err: cpal::StreamError) {
    tracing::error!(%err, "audio stream error");
}

fn push_interleaved_mono(
    data: &[f32],
    channels: usize,
    prod: &mut impl Producer<Item = f32>,
    status: &EngineStatus,
) {
    if channels == 0 {
        return;
    }
    let frames = data.len() / channels;
    for f in 0..frames {
        let mut sum = 0.0f32;
        for c in 0..channels {
            sum += data[f * channels + c];
        }
        let mono = sum / channels as f32;
        if prod.try_push(mono).is_err() {
            status.overruns.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn pop_mono_to_interleaved(
    data: &mut [f32],
    channels: usize,
    cons: &mut impl Consumer<Item = f32>,
    status: &EngineStatus,
) {
    if channels == 0 {
        return;
    }
    let frames = data.len() / channels;
    for f in 0..frames {
        let sample = match cons.try_pop() {
            Some(s) => s,
            None => {
                status.underruns.fetch_add(1, Ordering::Relaxed);
                0.0
            }
        };
        for c in 0..channels {
            data[f * channels + c] = sample;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn process_loop(
    mut cap_cons: impl Consumer<Item = f32> + Observer + Send + 'static,
    mut play_prod: impl Producer<Item = f32> + Send + 'static,
    params: Arc<LiveParams>,
    meters: Arc<LevelMeters>,
    status: Arc<EngineStatus>,
    stop: Arc<AtomicBool>,
    in_rate: u32,
    out_rate: u32,
) {
    let mut chain = DspChain::new(params);
    let mut work = vec![0.0f32; PROCESS_CHUNK];

    // Optional resamplers around the 48 kHz processing domain.
    let need_in_resample = in_rate != PROCESS_RATE;
    let need_out_resample = out_rate != PROCESS_RATE;

    let mut in_resampler = if need_in_resample {
        match FftFixedInOut::<f32>::new(in_rate as usize, PROCESS_RATE as usize, PROCESS_CHUNK, 1) {
            Ok(r) => Some(r),
            Err(err) => {
                tracing::error!(%err, "input resampler init failed");
                *status.last_error.lock() = Some(format!("input resampler: {err}"));
                None
            }
        }
    } else {
        None
    };

    let mut out_resampler = if need_out_resample {
        match FftFixedInOut::<f32>::new(PROCESS_RATE as usize, out_rate as usize, PROCESS_CHUNK, 1) {
            Ok(r) => Some(r),
            Err(err) => {
                tracing::error!(%err, "output resampler init failed");
                *status.last_error.lock() = Some(format!("output resampler: {err}"));
                None
            }
        }
    } else {
        None
    };

    let mut in_buf = vec![0.0f32; PROCESS_CHUNK * 4];
    let mut in_pending: Vec<f32> = Vec::with_capacity(PROCESS_CHUNK * 4);
    let mut out_pending: Vec<f32> = Vec::with_capacity(PROCESS_CHUNK * 4);

    // Preallocate resampler buffers if needed.
    let mut in_res_in = vec![vec![0.0f32; 0]];
    let mut in_res_out = vec![vec![0.0f32; 0]];
    if let Some(ref r) = in_resampler {
        in_res_in[0] = vec![0.0; r.input_frames_max()];
        in_res_out[0] = vec![0.0; r.output_frames_max()];
    }
    let mut out_res_in = vec![vec![0.0f32; PROCESS_CHUNK]];
    let mut out_res_out = vec![vec![0.0f32; 0]];
    if let Some(ref r) = out_resampler {
        out_res_out[0] = vec![0.0; r.output_frames_max()];
    }

    while !stop.load(Ordering::SeqCst) {
        // Pull available capture samples.
        let available = cap_cons.occupied_len();
        if available == 0 {
            thread::sleep(Duration::from_millis(1));
            continue;
        }
        let to_read = available.min(in_buf.len());
        let n = cap_cons.pop_slice(&mut in_buf[..to_read]);
        in_pending.extend_from_slice(&in_buf[..n]);

        // Resample into 48 kHz domain (or pass through).
        if let Some(ref mut res) = in_resampler {
            let need = res.input_frames_next();
            while in_pending.len() >= need {
                in_res_in[0][..need].copy_from_slice(&in_pending[..need]);
                in_pending.drain(..need);
                if let Ok((_i, o)) =
                    res.process_into_buffer(&in_res_in, &mut in_res_out, None)
                {
                    let produced = &in_res_out[0][..o];
                    meters.push_input(produced);
                    // Process in PROCESS_CHUNK blocks when possible; also handle remainder via work buffer path.
                    out_pending.extend_from_slice(produced);
                }
                let need = res.input_frames_next();
                if need == 0 {
                    break;
                }
            }
            // Drain out_pending through DSP in fixed chunks.
            while out_pending.len() >= PROCESS_CHUNK {
                work.copy_from_slice(&out_pending[..PROCESS_CHUNK]);
                out_pending.drain(..PROCESS_CHUNK);
                chain.process(&mut work);
                push_processed(
                    &work,
                    &mut play_prod,
                    &mut out_resampler,
                    &mut out_res_in,
                    &mut out_res_out,
                    &meters,
                    &status,
                );
            }
        } else {
            meters.push_input(&in_pending);
            while in_pending.len() >= PROCESS_CHUNK {
                work.copy_from_slice(&in_pending[..PROCESS_CHUNK]);
                in_pending.drain(..PROCESS_CHUNK);
                chain.process(&mut work);
                push_processed(
                    &work,
                    &mut play_prod,
                    &mut out_resampler,
                    &mut out_res_in,
                    &mut out_res_out,
                    &meters,
                    &status,
                );
            }
        }
    }
}

fn push_processed(
    work: &[f32],
    play_prod: &mut impl Producer<Item = f32>,
    out_resampler: &mut Option<FftFixedInOut<f32>>,
    out_res_in: &mut [Vec<f32>],
    out_res_out: &mut [Vec<f32>],
    meters: &LevelMeters,
    status: &EngineStatus,
) {
    meters.push_output(work);

if let Some(res) = out_resampler {
        // Feed fixed PROCESS_CHUNK frames.
        if out_res_in[0].len() < work.len() {
            out_res_in[0].resize(work.len(), 0.0);
        }
        out_res_in[0][..work.len()].copy_from_slice(work);
        // rubato FftFixedInOut expects exact input_frames_next.
        let need = res.input_frames_next();
        if work.len() != need {
            // Should be PROCESS_CHUNK consistently; if not, skip carefully.
            tracing::warn!(need, got = work.len(), "output resampler size mismatch");
        }
        if let Ok((_i, o)) = res.process_into_buffer(out_res_in, out_res_out, None) {
            for &s in &out_res_out[0][..o] {
                if play_prod.try_push(s).is_err() {
                    status.overruns.fetch_add(1, Ordering::Relaxed);
                    break;
                }
            }
        }
    } else {
        for &s in work {
            if play_prod.try_push(s).is_err() {
                status.overruns.fetch_add(1, Ordering::Relaxed);
                break;
            }
        }
    }
}