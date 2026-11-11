use super::engine::EngineStatus;
use ringbuf::traits::{Consumer, Producer};
use std::sync::atomic::Ordering;

/// Explicit conversions preserve the existing asymmetric PCM scaling.
pub(super) trait CaptureSample: Copy {
    fn to_mono_sample(self) -> f32;
}

impl CaptureSample for f32 {
    fn to_mono_sample(self) -> f32 {
        self
    }
}

impl CaptureSample for i16 {
    fn to_mono_sample(self) -> f32 {
        self as f32 / 32768.0
    }
}

pub(super) trait PlaybackSample: Copy {
    fn from_mono_sample(sample: f32) -> Self;
    fn clear_unwritten(_samples: &mut [Self]) {}
}

impl PlaybackSample for f32 {
    fn from_mono_sample(sample: f32) -> Self {
        sample
    }
}

impl PlaybackSample for i16 {
    fn from_mono_sample(sample: f32) -> Self {
        (sample.clamp(-1.0, 1.0) * 32767.0) as i16
    }

    fn clear_unwritten(samples: &mut [Self]) {
        samples.fill(0);
    }
}

pub(super) fn capture_mono<T: CaptureSample>(
    data: &[T],
    channels: usize,
    producer: &mut impl Producer<Item = f32>,
    status: &EngineStatus,
) {
    if channels == 0 {
        return;
    }
    for frame in data.chunks_exact(channels) {
        let sum = frame
            .iter()
            .fold(0.0, |sum, sample| sum + sample.to_mono_sample());
        if producer.try_push(sum / channels as f32).is_err() {
            status.overruns.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub(super) fn render_interleaved<T: PlaybackSample>(
    data: &mut [T],
    channels: usize,
    consumer: &mut impl Consumer<Item = f32>,
    status: &EngineStatus,
    gain: f32,
    silence: bool,
) {
    if channels == 0 {
        T::clear_unwritten(data);
        return;
    }
    let complete = data.len() / channels * channels;
    let (data, trailing) = data.split_at_mut(complete);
    // The original i16 conversion used a zeroed scratch buffer; f32 output
    // left incomplete frames untouched. Retain both without scratch allocation.
    T::clear_unwritten(trailing);
    for frame in data.chunks_exact_mut(channels) {
        let sample = if silence {
            // Disabled monitoring still drains the ring, without counting underruns.
            let _ = consumer.try_pop();
            0.0
        } else {
            match consumer.try_pop() {
                Some(sample) => sample * gain,
                None => {
                    status.underruns.fetch_add(1, Ordering::Relaxed);
                    0.0
                }
            }
        };
        frame.fill(T::from_mono_sample(sample));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ringbuf::{
        HeapRb,
        traits::{Observer, Split},
    };

    #[test]
    fn pcm_scaling_and_clipping_match_original() {
        assert_eq!(i16::MIN.to_mono_sample(), -1.0);
        assert_eq!(i16::MAX.to_mono_sample(), 32767.0 / 32768.0);
        assert_eq!(i16::from_mono_sample(-2.0), -32767);
        assert_eq!(i16::from_mono_sample(2.0), 32767);
        assert_eq!(i16::from_mono_sample(0.5), 16383);
        assert_eq!(f32::from_mono_sample(2.0), 2.0);
    }

    #[test]
    fn capture_mixes_channels_and_counts_each_overrun() {
        let status = EngineStatus::new();
        let (mut producer, mut consumer) = HeapRb::<f32>::new(1).split();
        capture_mono(&[0.25, 0.75, 0.0, 0.5, 0.5, 0.5], 2, &mut producer, &status);
        assert_eq!(consumer.try_pop(), Some(0.5));
        assert_eq!(status.overruns.load(Ordering::Relaxed), 2);
        capture_mono(&[i16::MIN, 0], 2, &mut producer, &status);
        assert_eq!(consumer.try_pop(), Some(-0.5));
    }

    #[test]
    fn playback_duplicates_mono_applies_gain_and_counts_frames() {
        let status = EngineStatus::new();
        let (mut producer, mut consumer) = HeapRb::<f32>::new(2).split();
        producer.try_push(0.25).unwrap();
        let mut output = [1.0; 6];
        render_interleaved(&mut output, 2, &mut consumer, &status, 2.0, false);
        assert_eq!(output, [0.5, 0.5, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(status.underruns.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn disabled_monitor_drains_without_underruns() {
        let status = EngineStatus::new();
        let (mut producer, mut consumer) = HeapRb::<f32>::new(2).split();
        producer.push_slice(&[0.25, 0.5]);
        let mut output = [1i16; 8];
        render_interleaved(&mut output, 2, &mut consumer, &status, 4.0, true);
        assert_eq!(output, [0; 8]);
        assert_eq!(consumer.occupied_len(), 0);
        assert_eq!(status.underruns.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn zero_channels_and_incomplete_frames_keep_original_semantics() {
        let status = EngineStatus::new();
        let (mut producer, mut consumer) = HeapRb::<f32>::new(4).split();
        capture_mono(&[0.5], 0, &mut producer, &status);
        assert_eq!(consumer.occupied_len(), 0);
        capture_mono(&[0.25, 0.75, 1.0], 2, &mut producer, &status);
        assert_eq!(consumer.occupied_len(), 1);
        let mut output = [9.0; 3];
        render_interleaved(&mut output, 0, &mut consumer, &status, 1.0, false);
        assert_eq!(output, [9.0; 3]);
        render_interleaved(&mut output, 2, &mut consumer, &status, 1.0, false);
        assert_eq!(output, [0.5, 0.5, 9.0]);
        let mut output = [9i16; 3];
        render_interleaved(&mut output, 0, &mut consumer, &status, 1.0, false);
        assert_eq!(output, [0; 3]);
        producer.try_push(0.5).unwrap();
        output.fill(9);
        render_interleaved(&mut output, 2, &mut consumer, &status, 1.0, false);
        assert_eq!(output, [16383, 16383, 0]);
    }
}
