use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};

use anyhow::{Context, Result};
use cpal::Stream;
use cpal::traits::{DeviceTrait, StreamTrait};
use parking_lot::Mutex;
use ringbuf::HeapRb;
use ringbuf::traits::{Producer, Split};

use super::devices::{find_input_device, find_monitor_device, find_output_device};
use super::meters::LevelMeters;
use super::processing::{ProcessingContext, ProcessingRates, process_loop};
use super::{PROCESS_RATE, RING_FRAMES, streams};
use crate::config::LiveParams;

pub struct EngineStatus {
    pub underruns: AtomicU64,
    pub overruns: AtomicU64,
    pub last_error: Mutex<Option<String>>,
    pub input_name: Mutex<String>,
    pub output_name: Mutex<String>,
    pub monitor_name: Mutex<String>,
    pub input_rate: AtomicU64,
    pub output_rate: AtomicU64,
    pub monitor_rate: AtomicU64,
}

impl EngineStatus {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            underruns: AtomicU64::new(0),
            overruns: AtomicU64::new(0),
            last_error: Mutex::new(None),
            input_name: Mutex::new(String::new()),
            output_name: Mutex::new(String::new()),
            monitor_name: Mutex::new(String::new()),
            input_rate: AtomicU64::new(0),
            output_rate: AtomicU64::new(0),
            monitor_rate: AtomicU64::new(0),
        })
    }
}

struct RunningStreams {
    _input: Stream,
    _output: Stream,
    _monitor: Option<Stream>,
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

    pub fn start(
        &self,
        input_name: Option<&str>,
        output_name: Option<&str>,
        monitor_name: Option<&str>,
    ) -> Result<()> {
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

        let monitor_device = match find_monitor_device(monitor_name, Some(out_name.as_str())) {
            Ok(d) => {
                let mon_name = d.name().unwrap_or_default();
                if mon_name == out_name {
                    tracing::warn!(
                        %mon_name,
                        "monitor device matches cable output; monitor disabled"
                    );
                    *self.status.monitor_name.lock() = String::new();
                    None
                } else {
                    *self.status.monitor_name.lock() = mon_name;
                    Some(d)
                }
            }
            Err(err) => {
                tracing::warn!(%err, "monitor device unavailable");
                *self.status.monitor_name.lock() = String::new();
                None
            }
        };

        tracing::info!(
            %in_name,
            %out_name,
            monitor = %self.status.monitor_name.lock(),
            "starting audio engine"
        );

        let in_config = streams::input_config(&input_device)?;
        let out_config = streams::output_config(&output_device)?;
        let in_rate = in_config.rate();
        let out_rate = out_config.rate();
        self.status
            .input_rate
            .store(in_rate as u64, Ordering::Relaxed);
        self.status
            .output_rate
            .store(out_rate as u64, Ordering::Relaxed);

        let mon_cfg = if let Some(device) = &monitor_device {
            let cfg = streams::output_config(device).ok();
            if let Some(cfg) = &cfg {
                self.status
                    .monitor_rate
                    .store(cfg.rate() as u64, Ordering::Relaxed);
            }
            cfg
        } else {
            self.status.monitor_rate.store(0, Ordering::Relaxed);
            None
        };
        tracing::info!(
            in_rate, out_rate,
            in_channels = in_config.channels(),
            out_channels = out_config.channels(),
            mon_rate = mon_cfg.as_ref().map(|cfg| cfg.rate()).unwrap_or(0),
            in_sample_format = ?in_config.format,
            out_sample_format = ?out_config.format,
            "stream configuration"
        );

        let (cap_prod, cap_cons) = HeapRb::<f32>::new(RING_FRAMES).split();
        let (mut play_prod, play_cons) = HeapRb::<f32>::new(RING_FRAMES).split();
        let (mut mon_prod_opt, mon_cons_opt) = if mon_cfg.is_some() {
            let (p, c) = HeapRb::<f32>::new(RING_FRAMES).split();
            (Some(p), Some(c))
        } else {
            (None, None)
        };

        let prime = (PROCESS_RATE as usize) / 50; // 20 ms
        for _ in 0..prime {
            let _ = play_prod.try_push(0.0);
            if let Some(ref mut mp) = mon_prod_opt {
                let _ = mp.try_push(0.0);
            }
        }

        let stop = Arc::new(AtomicBool::new(false));
        let status = self.status.clone();
        let meters = self.meters.clone();
        let params = self.params.clone();

        let input_stream =
            streams::build_input(&input_device, &in_config, cap_prod, status.clone())
                .context("build input stream")?;
        let output_stream =
            streams::build_output(&output_device, &out_config, play_cons, status.clone(), None)
                .context("build output stream")?;
        let monitor_stream = match (monitor_device, mon_cfg, mon_cons_opt) {
            (Some(device), Some(config), Some(consumer)) => Some(streams::build_output(
                &device,
                &config,
                consumer,
                status.clone(),
                Some(params.clone()),
            )?),
            _ => None,
        };

        input_stream.play().context("play input")?;
        output_stream.play().context("play output")?;
        if let Some(ref mon) = monitor_stream {
            mon.play().context("play monitor")?;
        }

        let stop_flag = stop.clone();
        let status_proc = status.clone();
        let mon_rate = self.status.monitor_rate.load(Ordering::Relaxed) as u32;
        let proc_handle = thread::Builder::new()
            .name("mic-dsp".into())
            .spawn(move || {
                process_loop(
                    cap_cons,
                    play_prod,
                    mon_prod_opt,
                    ProcessingContext {
                        params,
                        meters,
                        status: status_proc,
                        stop: stop_flag,
                        rates: ProcessingRates {
                            input: in_rate,
                            output: out_rate,
                            monitor: mon_rate,
                        },
                    },
                );
            })
            .context("spawn processing thread")?;

        self.params.running.store(true, Ordering::Relaxed);
        *self.running.lock() = Some(RunningStreams {
            _input: input_stream,
            _output: output_stream,
            _monitor: monitor_stream,
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
        }
        self.params.tuning.stop();
        self.params
            .limiter_reduction_db
            .store(0.0, Ordering::Relaxed);
        self.params
            .replay_limiter_reduction_db
            .store(0.0, Ordering::Relaxed);
        self.params.running.store(false, Ordering::Relaxed);
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.stop();
    }
}
