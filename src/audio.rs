use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

pub struct AudioCapture {
    _stream: Stream,
}

pub struct AudioPlayback {
    _stream: Stream,
}

pub fn start_capture(mic_tx: mpsc::Sender<Vec<i16>>) -> Result<(AudioCapture, StreamConfig)> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow::anyhow!("No input audio device found"))?;

    let supported = device.default_input_config()?;
    let sample_format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let returned_config = config.clone();

    let stream = match sample_format {
        SampleFormat::I16 => device.build_input_stream(
            &config,
            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                let _ = mic_tx.try_send(data.to_vec());
            },
            |err| eprintln!("Input stream error: {err}"),
            None,
        )?,
        SampleFormat::F32 => device.build_input_stream(
            &config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let converted: Vec<i16> = data.iter().map(|&s| (s * 32767.0) as i16).collect();
                let _ = mic_tx.try_send(converted);
            },
            |err| eprintln!("Input stream error: {err}"),
            None,
        )?,
        _ => anyhow::bail!("Unsupported sample format: {sample_format}"),
    };

    stream.play()?;
    Ok((AudioCapture { _stream: stream }, returned_config))
}

pub fn start_playback(
    playback_buffer: Arc<Mutex<VecDeque<i16>>>,
) -> Result<(AudioPlayback, StreamConfig)> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| anyhow::anyhow!("No output audio device found"))?;

    let supported = device.default_output_config()?;
    let sample_format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let returned_config = config.clone();

    let stream = match sample_format {
        SampleFormat::I16 => {
            device.build_output_stream(
                &config,
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    let mut buf = playback_buffer.lock().unwrap();
                    for sample in data.iter_mut() {
                        *sample = buf.pop_front().unwrap_or(0);
                    }
                },
                |err| eprintln!("Output stream error: {err}"),
                None,
            )?
        }
        SampleFormat::F32 => {
            device.build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let mut buf = playback_buffer.lock().unwrap();
                    for sample in data.iter_mut() {
                        *sample = buf.pop_front().unwrap_or(0) as f32 / 32767.0;
                    }
                },
                |err| eprintln!("Output stream error: {err}"),
                None,
            )?
        }
        _ => anyhow::bail!("Unsupported output sample format: {sample_format}"),
    };

    stream.play()?;
    Ok((AudioPlayback { _stream: stream }, returned_config))
}

/// Convert stereo (or multi-channel) to mono by averaging channels.
pub fn stereo_to_mono(input: &[i16], channels: u16) -> Vec<i16> {
    if channels == 1 {
        return input.to_vec();
    }
    input
        .chunks(channels as usize)
        .map(|frame| {
            let sum: i32 = frame.iter().map(|&s| s as i32).sum();
            (sum / channels as i32) as i16
        })
        .collect()
}

/// Resample audio using linear interpolation.
pub fn resample(input: &[i16], from_rate: u32, to_rate: u32) -> Vec<i16> {
    if from_rate == to_rate {
        return input.to_vec();
    }

    let ratio = from_rate as f64 / to_rate as f64;
    let output_len = (input.len() as f64 / ratio) as usize;
    let mut output = Vec::with_capacity(output_len);

    for i in 0..output_len {
        let src_pos = i as f64 * ratio;
        let idx = src_pos as usize;
        let frac = src_pos - idx as f64;

        if idx + 1 < input.len() {
            let a = input[idx] as f64;
            let b = input[idx + 1] as f64;
            output.push((a + (b - a) * frac) as i16);
        } else if idx < input.len() {
            output.push(input[idx]);
        }
    }
    output
}

/// Expand mono samples to multi-channel (duplicate sample across channels).
pub fn mono_to_channels(input: &[i16], channels: u16) -> Vec<i16> {
    if channels == 1 {
        return input.to_vec();
    }
    input
        .iter()
        .flat_map(|&s| std::iter::repeat_n(s, channels as usize))
        .collect()
}

/// Stream mic audio continuously to the outgoing WebSocket channel.
pub async fn mic_to_events(
    mut mic_rx: mpsc::Receiver<Vec<i16>>,
    outgoing_tx: mpsc::Sender<String>,
    input_sample_rate: u32,
    input_channels: u16,
) -> Result<()> {
    use base64::Engine;
    use crate::protocol::ClientEvent;

    while let Some(raw_samples) = mic_rx.recv().await {
        let mono = stereo_to_mono(&raw_samples, input_channels);
        let resampled = resample(&mono, input_sample_rate, 24000);

        let bytes: Vec<u8> = resampled.iter().flat_map(|s| s.to_le_bytes()).collect();
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);

        let event = ClientEvent::InputAudioBufferAppend { audio: b64 };
        let json = serde_json::to_string(&event)?;
        outgoing_tx.send(json).await?;
    }
    Ok(())
}
