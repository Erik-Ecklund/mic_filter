use nnnoiseless::DenoiseState;

const FRAME: usize = DenoiseState::FRAME_SIZE; // 480
const I16_SCALE: f32 = 32768.0;

/// Streaming RNNoise wrapper. Accepts normalized ±1.0 mono samples at 48 kHz.
pub struct NoiseSuppressor {
    state: Box<DenoiseState<'static>>,
    pending: Vec<f32>,
    frame_in: [f32; FRAME],
    frame_out: [f32; FRAME],
    out_queue: Vec<f32>,
    first_frame: bool,
}

impl NoiseSuppressor {
    pub fn new() -> Self {
        Self {
            state: DenoiseState::new(),
            pending: Vec::with_capacity(FRAME),
            frame_in: [0.0; FRAME],
            frame_out: [0.0; FRAME],
            out_queue: Vec::with_capacity(FRAME * 2),
            first_frame: true,
        }
    }

    /// Process arbitrary-length mono input into `output` (same length).
    /// Maintains internal latency of one RNNoise frame after the first call settles.
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) {
        debug_assert_eq!(input.len(), output.len());

        for &sample in input {
            self.pending.push(sample);
            while self.pending.len() >= FRAME {
                for i in 0..FRAME {
                    self.frame_in[i] = self.pending[i] * I16_SCALE;
                }
                self.pending.drain(..FRAME);
                self.state
                    .process_frame(&mut self.frame_out, &self.frame_in);
                if self.first_frame {
                    // Discard first frame (fade-in artifacts).
                    self.first_frame = false;
                    // Compensate latency with silence so in/out lengths stay aligned.
                    for _ in 0..FRAME {
                        self.out_queue.push(0.0);
                    }
                } else {
                    for &s in &self.frame_out {
                        self.out_queue.push((s / I16_SCALE).clamp(-1.0, 1.0));
                    }
                }
            }
        }

        for out in output.iter_mut() {
            *out = self.out_queue.pop_front_or_zero();
        }
    }

    pub fn reset(&mut self) {
        self.state = DenoiseState::new();
        self.pending.clear();
        self.out_queue.clear();
        self.first_frame = true;
    }
}

trait PopFrontOrZero {
    fn pop_front_or_zero(&mut self) -> f32;
}

impl PopFrontOrZero for Vec<f32> {
    fn pop_front_or_zero(&mut self) -> f32 {
        if self.is_empty() {
            0.0
        } else {
            self.remove(0)
        }
    }
}

impl Default for NoiseSuppressor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_preserves_length_and_bounds() {
        let mut ns = NoiseSuppressor::new();
        let input: Vec<f32> = (0..1000)
            .map(|i| ((i as f32) * 0.01).sin() * 0.2)
            .collect();
        let mut output = vec![0.0; input.len()];
        ns.process(&input, &mut output);
        assert_eq!(output.len(), input.len());
        assert!(output.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
    }

    #[test]
    fn first_frame_emits_silence_padding() {
        let mut ns = NoiseSuppressor::new();
        let input = vec![0.1f32; FRAME];
        let mut output = vec![0.0; FRAME];
        ns.process(&input, &mut output);
        assert!(output.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn subsequent_frames_produce_energy_for_tone() {
        let mut ns = NoiseSuppressor::new();
        let mut input = Vec::with_capacity(FRAME * 3);
        for i in 0..(FRAME * 3) {
            let t = i as f32 / 48_000.0;
            input.push((2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.3);
        }
        let mut output = vec![0.0; input.len()];
        ns.process(&input, &mut output);
        let tail = &output[FRAME..];
        let energy: f32 = tail.iter().map(|s| s * s).sum();
        assert!(energy > 0.0, "expected energy after latency padding");
    }

    #[test]
    fn reset_clears_state() {
        let mut ns = NoiseSuppressor::new();
        let input = vec![0.2f32; FRAME * 2];
        let mut output = vec![0.0; input.len()];
        ns.process(&input, &mut output);
        ns.reset();
        assert!(ns.pending.is_empty());
        assert!(ns.out_queue.is_empty());
        assert!(ns.first_frame);
    }
}
