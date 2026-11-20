pub(super) fn tone(frequency: f32, amplitude: f32, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| {
            (std::f64::consts::TAU * frequency as f64 * i as f64 / 48_000.0).sin() as f32
                * amplitude
        })
        .collect()
}

pub(super) fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / samples.len() as f64).sqrt() as f32
}

pub(super) fn amplitude_at(samples: &[f32], frequency: f32) -> f32 {
    let (mut sine, mut cosine) = (0.0, 0.0);
    for (i, &sample) in samples.iter().enumerate() {
        let phase = std::f64::consts::TAU * frequency as f64 * i as f64 / 48_000.0;
        sine += sample as f64 * phase.sin();
        cosine += sample as f64 * phase.cos();
    }
    (2.0 * sine.hypot(cosine) / samples.len() as f64) as f32
}
