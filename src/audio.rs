//! Small procedural radar cues. No decoded sound buffers or audio framework beyond output.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{
    Arc,
    atomic::{AtomicU8, AtomicU32, Ordering},
};

#[derive(Default)]
struct State {
    flags: AtomicU8,
    volume: AtomicU32,
}
pub struct RadarAudio {
    state: Arc<State>,
    _stream: Option<cpal::Stream>,
}

impl Default for RadarAudio {
    fn default() -> Self {
        Self::new()
    }
}

impl RadarAudio {
    pub fn new() -> Self {
        let state = Arc::new(State::default());
        let stream = match open(Arc::clone(&state)) {
            Ok(stream) => Some(stream),
            Err(error) => {
                log::warn!("Audio output unavailable: {error}");
                None
            }
        };
        Self {
            state,
            _stream: stream,
        }
    }
    pub fn update(&self, flags: u8, volume: u8) {
        self.state.flags.store(flags, Ordering::Relaxed);
        self.state.volume.store(volume as u32, Ordering::Relaxed);
    }
}

fn open(state: Arc<State>) -> Result<cpal::Stream, Box<dyn std::error::Error>> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("no audio device")?;
    let config = device.default_output_config()?;
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => stream::<f32>(&device, &config.into(), state)?,
        cpal::SampleFormat::I16 => stream::<i16>(&device, &config.into(), state)?,
        cpal::SampleFormat::U16 => stream::<u16>(&device, &config.into(), state)?,
        _ => return Err("unsupported audio sample format".into()),
    };
    stream.play()?;
    Ok(stream)
}

fn stream<T: cpal::SizedSample + cpal::FromSample<f32>>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    state: Arc<State>,
) -> Result<cpal::Stream, cpal::BuildStreamError> {
    let channels = config.channels as usize;
    let rate = config.sample_rate.0 as f32;
    let mut oscillator = Tone::default();
    device.build_output_stream(
        config,
        move |output: &mut [T], _| {
            let flags = state.flags.load(Ordering::Relaxed);
            let volume = state.volume.load(Ordering::Relaxed) as f32 / 100.;
            for frame in output.chunks_mut(channels) {
                let sample = oscillator.sample(flags, volume, rate);
                for channel in frame {
                    *channel = T::from_sample(sample);
                }
            }
        },
        |error| log::warn!("Audio stream: {error}"),
        None,
    )
}

#[derive(Default)]
struct Tone {
    phase: f32,
    time: f32,
    envelope: f32,
}
impl Tone {
    fn sample(&mut self, flags: u8, volume: f32, rate: f32) -> f32 {
        self.time = (self.time + 1. / rate) % 4.;
        let (frequency, on) = if flags & 4 != 0 {
            (1200., self.time % 0.2 < 0.1)
        } else if flags & 2 != 0 {
            (950., self.time % 0.45 < 0.2)
        } else if flags & 1 != 0 {
            (650., self.time % 0.8 < 0.12)
        } else {
            (650., false)
        };
        let target = if on { 1. } else { 0. };
        self.envelope += (target - self.envelope) * 0.005;
        self.phase = (self.phase + frequency / rate) % 1.;
        (self.phase * std::f32::consts::TAU).sin() * self.envelope * volume * 0.12
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cues_are_bounded_and_volume_zero_is_silent() {
        for flags in [1, 2, 4] {
            let mut tone = Tone::default();
            let mut energy = 0.;
            for _ in 0..48000 {
                let s = tone.sample(flags, 1., 48000.);
                assert!(s.abs() <= 0.121);
                energy += s * s;
            }
            assert!(energy > 1.);
            assert_eq!(tone.sample(flags, 0., 48000.), 0.);
        }
    }
}
