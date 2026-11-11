mod devices;
mod engine;
mod meters;
mod processing;
mod samples;
mod streams;

const PROCESS_RATE: u32 = 48_000;
const PROCESS_CHUNK: usize = 480; // RNNoise frame / 10 ms
const RING_FRAMES: usize = PROCESS_RATE as usize / 2;

pub use devices::{DeviceInfo, list_input_devices, list_output_devices};
pub use engine::AudioEngine;
pub use meters::{LevelMeters, db_to_meter};
