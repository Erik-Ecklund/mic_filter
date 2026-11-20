/// Clamp finite parameters, using a safe fallback for NaN and infinity.
pub(super) fn finite_clamp(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

/// Convert decibels to a linear amplitude multiplier.
#[inline]
pub(crate) fn db_to_lin(db: f32) -> f32 {
    10.0f32.powf(db * 0.05)
}

#[inline]
pub(crate) fn lin_to_db(x: f32) -> f32 {
    20.0 * x.log10()
}

/// One-pole smoothing coefficient, clamped to at least one sample.
#[inline]
pub(super) fn time_to_coeff(ms: f32, sample_rate: f32) -> f32 {
    let t = (ms.max(0.01) * 0.001).max(1.0 / sample_rate);
    (-1.0 / (t * sample_rate)).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decibels_roundtrip() {
        assert_eq!(db_to_lin(0.0), 1.0);
        for db in [-80.0, -20.0, -6.0, 6.0, 24.0] {
            assert!((lin_to_db(db_to_lin(db)) - db).abs() < 1e-4);
        }
    }

    #[test]
    fn smoothing_clamps_to_one_sample_and_increases_with_time() {
        let minimum = (-1.0f32).exp();
        assert_eq!(time_to_coeff(-1.0, 48_000.0), minimum);
        assert_eq!(time_to_coeff(0.0, 48_000.0), minimum);
        assert!(time_to_coeff(100.0, 48_000.0) > time_to_coeff(1.0, 48_000.0));
        assert!(time_to_coeff(100.0, 48_000.0) < 1.0);
    }
}
