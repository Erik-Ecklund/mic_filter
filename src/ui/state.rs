use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::audio::{AudioEngine, DeviceInfo, LevelMeters, list_input_devices, list_output_devices};
use crate::config::{AppConfig, LiveParams, save_config};

pub struct AppState {
    pub config: AppConfig,
    pub params: Arc<LiveParams>,
    pub engine: Rc<AudioEngine>,
    pub meters: Arc<LevelMeters>,
    pub inputs: Vec<DeviceInfo>,
    pub outputs: Vec<DeviceInfo>,
    pub status_message: String,
    pub quit_requested: bool,
    pub(super) next_filter_id: u64,
}

impl AppState {
    pub fn new(
        config: AppConfig,
        params: Arc<LiveParams>,
        engine: Rc<AudioEngine>,
        meters: Arc<LevelMeters>,
    ) -> Self {
        let inputs = list_input_devices().unwrap_or_default();
        let outputs = list_output_devices().unwrap_or_default();
        let next_filter_id = config
            .filters
            .iter()
            .map(|f| f.id)
            .max()
            .unwrap_or(0)
            .wrapping_add(1);
        Self {
            config,
            params,
            engine,
            meters,
            inputs,
            outputs,
            status_message: String::new(),
            quit_requested: false,
            next_filter_id,
        }
    }

    pub fn refresh_devices(&mut self) {
        self.inputs = list_input_devices().unwrap_or_default();
        self.outputs = list_output_devices().unwrap_or_default();
    }

    pub fn start_engine(&mut self) {
        let input = self.config.input_device.as_deref();
        let output = self.config.output_device.as_deref();
        let monitor = self.config.monitor_device.as_deref();
        match self.engine.start(input, output, monitor) {
            Ok(()) => {
                let st = self.engine.status();
                let mon = st.monitor_name.lock().clone();
                self.status_message = if mon.is_empty() {
                    format!(
                        "Running: {} → {} (no monitor device)",
                        st.input_name.lock(),
                        st.output_name.lock()
                    )
                } else {
                    format!(
                        "Running: {} → {} | monitor: {}",
                        st.input_name.lock(),
                        st.output_name.lock(),
                        mon
                    )
                };
            }
            Err(err) => {
                self.status_message = format!("Start failed: {err:#}");
                tracing::error!(%err, "engine start failed");
            }
        }
    }

    pub fn stop_engine(&mut self) {
        self.engine.stop();
        self.status_message = "Stopped".into();
    }

    pub fn toggle_mute(&self) {
        self.params.muted.fetch_xor(true, Ordering::Relaxed);
    }

    pub fn set_monitor_enabled(&mut self, enabled: bool) {
        self.params
            .monitor_enabled
            .store(enabled, Ordering::Relaxed);
        self.config.monitor_enabled = enabled;
    }

    pub fn toggle_monitor(&mut self) {
        self.set_monitor_enabled(!self.params.monitor_enabled.load(Ordering::Relaxed));
    }

    pub(super) fn apply_devices(&mut self) {
        if self.engine.is_running() {
            self.start_engine();
        }
        self.persist();
    }

    pub fn persist(&mut self) {
        self.params.snapshot_into(&mut self.config);
        if let Err(err) = save_config(&self.config) {
            self.status_message = format!("Save failed: {err:#}");
        } else {
            self.status_message = "Settings saved".into();
        }
    }
}
