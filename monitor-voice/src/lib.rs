//! Native voice for monitor-agent — a plumbing-first [`VoiceEngine`] seam plus
//! microphone capture.
//!
//! **V1b-1 (this):** a [`StubVoiceEngine`] keeps the chat loop working, and
//! [`MicCapture`] opens the default mic (via `cpal`) to stream RMS levels for
//! the voice waveform and accumulate samples for transcription. Real native
//! engines (whisper.cpp STT, piper/onnx TTS) plug in behind [`VoiceEngine`]
//! later — the GUI and the rest of the app never change.

use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// A pluggable speech engine. The active engine turns captured audio into text
/// and text into speech; the stub keeps the loop visible before native engines.
pub trait VoiceEngine: Send {
    /// Transcribe captured mono `samples` at `sample_rate` Hz to text.
    fn transcribe(&mut self, samples: &[f32], sample_rate: u32) -> anyhow::Result<String>;
    /// Speak `text` aloud (blocking). A real engine plays audio; the stub no-ops.
    fn speak(&mut self, text: &str) -> anyhow::Result<()>;
}

/// Plumbing-first placeholder engine: reports what it "heard" instead of running
/// a model, so the press-to-talk → chat loop works before native STT/TTS land.
#[derive(Debug, Default, Clone, Copy)]
pub struct StubVoiceEngine;

impl VoiceEngine for StubVoiceEngine {
    fn transcribe(&mut self, samples: &[f32], sample_rate: u32) -> anyhow::Result<String> {
        let secs = if sample_rate == 0 {
            0.0
        } else {
            samples.len() as f32 / sample_rate as f32
        };
        Ok(format!(
            "(stub STT) heard {secs:.1}s of audio — native whisper wires in next."
        ))
    }

    fn speak(&mut self, _text: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Root-mean-square level of a sample chunk, clamped to `0.0..=1.0`.
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt().clamp(0.0, 1.0)
}

/// An open microphone stream. Captured mono samples accumulate for later
/// transcription; per-callback RMS levels stream out via [`drain_rms`]. Dropping
/// it stops capture.
///
/// [`drain_rms`]: MicCapture::drain_rms
pub struct MicCapture {
    _stream: cpal::Stream,
    rms_rx: Receiver<f32>,
    samples: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
}

impl MicCapture {
    /// Open the default input device and start capturing.
    pub fn start() -> anyhow::Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| anyhow::anyhow!("no default input device"))?;
        let supported = device.default_input_config()?;
        let sample_rate = supported.sample_rate().0;
        let channels = supported.channels() as usize;
        let sample_format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();

        let samples: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
        let (tx, rms_rx) = std::sync::mpsc::channel::<f32>();
        let sink = samples.clone();

        let stream = match sample_format {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &config,
                move |data: &[f32], _: &_| capture_chunk(data, channels, &sink, &tx),
                on_stream_error,
                None,
            )?,
            cpal::SampleFormat::I16 => device.build_input_stream(
                &config,
                move |data: &[i16], _: &_| {
                    let f: Vec<f32> = data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                    capture_chunk(&f, channels, &sink, &tx);
                },
                on_stream_error,
                None,
            )?,
            other => anyhow::bail!("unsupported input sample format: {other:?}"),
        };
        stream.play()?;
        Ok(Self {
            _stream: stream,
            rms_rx,
            samples,
            sample_rate,
        })
    }

    /// Drain RMS levels produced since the last call (newest last).
    pub fn drain_rms(&self) -> Vec<f32> {
        self.rms_rx.try_iter().collect()
    }

    /// The capture sample rate (Hz).
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Take the accumulated mono samples, clearing the buffer.
    pub fn take_samples(&self) -> Vec<f32> {
        std::mem::take(&mut self.samples.lock().expect("samples mutex poisoned"))
    }
}

fn on_stream_error(err: cpal::StreamError) {
    tracing::warn!("mic stream error: {err}");
}

/// Downmix to mono, push the chunk's RMS, and accumulate samples (bounded).
fn capture_chunk(data: &[f32], channels: usize, sink: &Arc<Mutex<Vec<f32>>>, tx: &Sender<f32>) {
    let mono: Vec<f32> = if channels <= 1 {
        data.to_vec()
    } else {
        data.chunks(channels)
            .map(|c| c.iter().sum::<f32>() / channels as f32)
            .collect()
    };
    let _ = tx.send(rms(&mono));
    if let Ok(mut buf) = sink.lock() {
        buf.extend_from_slice(&mono);
        // Bound the buffer (~30s at 48 kHz) so a stuck session can't grow forever.
        const CAP: usize = 48_000 * 30;
        if buf.len() > CAP {
            let drop = buf.len() - CAP;
            buf.drain(0..drop);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rms_of_silence_is_zero() {
        assert_eq!(rms(&[0.0; 64]), 0.0);
        assert_eq!(rms(&[]), 0.0);
    }

    #[test]
    fn rms_of_full_scale_is_one() {
        assert!((rms(&[1.0, -1.0, 1.0, -1.0]) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn stub_engine_reports_duration() {
        let mut e = StubVoiceEngine;
        let text = e.transcribe(&[0.0; 16_000], 16_000).unwrap();
        assert!(text.contains("1.0s"));
        assert!(e.speak("hi").is_ok());
    }
}
