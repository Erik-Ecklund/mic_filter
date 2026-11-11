use std::sync::Arc;
use std::sync::atomic::Ordering;

use anyhow::{Context, Result};
use cpal::traits::DeviceTrait;
use cpal::{Device, SampleFormat, SizedSample, Stream, StreamConfig, SupportedStreamConfigRange};
use ringbuf::traits::{Consumer, Producer};

use super::samples::{CaptureSample, PlaybackSample, capture_mono, render_interleaved};
use super::{PROCESS_RATE, engine::EngineStatus};
use crate::config::LiveParams;
use crate::dsp::math::db_to_lin;

pub(super) struct DeviceConfig {
    pub config: StreamConfig,
    pub format: SampleFormat,
}

impl DeviceConfig {
    pub fn rate(&self) -> u32 {
        self.config.sample_rate.0
    }
    pub fn channels(&self) -> usize {
        self.config.channels as usize
    }
}

fn prefer_processing_rate(
    config: &mut StreamConfig,
    format: SampleFormat,
    ranges: impl Iterator<Item = SupportedStreamConfigRange>,
    output: bool,
) {
    if let Some(range) = ranges
        .filter(|range| {
            range.sample_format() == SampleFormat::F32 || range.sample_format() == format
        })
        .find(|range| {
            range.min_sample_rate().0 <= PROCESS_RATE && range.max_sample_rate().0 >= PROCESS_RATE
        })
    {
        config.sample_rate = cpal::SampleRate(PROCESS_RATE);
        config.channels = if output {
            range.channels().clamp(1, 2)
        } else {
            range.channels().min(2)
        };
    }
}

pub(super) fn input_config(device: &Device) -> Result<DeviceConfig> {
    let supported = device
        .default_input_config()
        .context("input default config")?;
    let mut selection = DeviceConfig {
        config: supported.config(),
        format: supported.sample_format(),
    };
    if let Ok(ranges) = device.supported_input_configs() {
        prefer_processing_rate(&mut selection.config, selection.format, ranges, false);
    }
    Ok(selection)
}

pub(super) fn output_config(device: &Device) -> Result<DeviceConfig> {
    let supported = device
        .default_output_config()
        .context("output default config")?;
    let mut selection = DeviceConfig {
        config: supported.config(),
        format: supported.sample_format(),
    };
    if let Ok(ranges) = device.supported_output_configs() {
        prefer_processing_rate(&mut selection.config, selection.format, ranges, true);
    }
    Ok(selection)
}

pub(super) fn build_input(
    device: &Device,
    selection: &DeviceConfig,
    producer: impl Producer<Item = f32> + Send + 'static,
    status: Arc<EngineStatus>,
) -> Result<Stream> {
    match selection.format {
        SampleFormat::F32 => input_stream::<f32>(device, selection, producer, status),
        SampleFormat::I16 => input_stream::<i16>(device, selection, producer, status),
        other => Err(anyhow::anyhow!(
            "unsupported input sample format: {other:?}"
        )),
    }
}

fn input_stream<T: CaptureSample + SizedSample>(
    device: &Device,
    selection: &DeviceConfig,
    mut producer: impl Producer<Item = f32> + Send + 'static,
    status: Arc<EngineStatus>,
) -> Result<Stream> {
    let channels = selection.channels();
    Ok(device.build_input_stream(
        &selection.config,
        move |data: &[T], _| capture_mono(data, channels, &mut producer, &status),
        stream_error,
        None,
    )?)
}

pub(super) fn build_output(
    device: &Device,
    selection: &DeviceConfig,
    consumer: impl Consumer<Item = f32> + Send + 'static,
    status: Arc<EngineStatus>,
    monitor: Option<Arc<LiveParams>>,
) -> Result<Stream> {
    match selection.format {
        SampleFormat::F32 => output_stream::<f32>(device, selection, consumer, status, monitor),
        SampleFormat::I16 => output_stream::<i16>(device, selection, consumer, status, monitor),
        other => {
            let label = if monitor.is_some() {
                "monitor"
            } else {
                "output"
            };
            Err(anyhow::anyhow!(
                "unsupported {label} sample format: {other:?}"
            ))
        }
    }
}

fn output_stream<T: PlaybackSample + SizedSample>(
    device: &Device,
    selection: &DeviceConfig,
    mut consumer: impl Consumer<Item = f32> + Send + 'static,
    status: Arc<EngineStatus>,
    monitor: Option<Arc<LiveParams>>,
) -> Result<Stream> {
    let channels = selection.channels();
    Ok(device.build_output_stream(
        &selection.config,
        move |data: &mut [T], _| {
            let (gain, silence) = match &monitor {
                Some(params) if params.monitor_enabled.load(Ordering::Relaxed) => (
                    db_to_lin(params.monitor_gain_db.load(Ordering::Relaxed)),
                    false,
                ),
                Some(_) => (0.0, true),
                None => (1.0, false),
            };
            render_interleaved(data, channels, &mut consumer, &status, gain, silence);
        },
        stream_error,
        None,
    )?)
}

fn stream_error(error: cpal::StreamError) {
    tracing::error!(%error, "audio stream error");
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpal::{BufferSize, SampleRate, SupportedBufferSize};

    fn range(channels: u16, min: u32, max: u32) -> SupportedStreamConfigRange {
        SupportedStreamConfigRange::new(
            channels,
            SampleRate(min),
            SampleRate(max),
            SupportedBufferSize::Unknown,
            SampleFormat::F32,
        )
    }

    #[test]
    fn configuration_prefers_processing_rate_and_limits_channels() {
        let mut cfg = StreamConfig {
            channels: 6,
            sample_rate: SampleRate(44_100),
            buffer_size: BufferSize::Default,
        };
        prefer_processing_rate(
            &mut cfg,
            SampleFormat::F32,
            [range(6, 44_100, 96_000)].into_iter(),
            true,
        );
        assert_eq!(cfg.sample_rate.0, PROCESS_RATE);
        assert_eq!(cfg.channels, 2);
    }

    #[test]
    fn configuration_keeps_device_defaults_when_rate_is_unavailable() {
        let mut cfg = StreamConfig {
            channels: 1,
            sample_rate: SampleRate(44_100),
            buffer_size: BufferSize::Default,
        };
        prefer_processing_rate(
            &mut cfg,
            SampleFormat::F32,
            [range(2, 44_100, 44_100)].into_iter(),
            false,
        );
        assert_eq!(cfg.sample_rate.0, 44_100);
        assert_eq!(cfg.channels, 1);
    }
}
