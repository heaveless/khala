use anyhow::Result;
use base64::Engine;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use crate::metrics::{self, PipelineMetrics};
use crate::protocol::ClientEvent;

pub struct AudioHandle {
    _stream: Stream,
}

// --- Device discovery ---

pub fn find_input(name: Option<&str>) -> Result<Device> {
    let host = cpal::default_host();
    find_device(name, host.input_devices()?, host.default_input_device(), "Input")
}

pub fn find_output(name: Option<&str>) -> Result<Device> {
    let host = cpal::default_host();
    find_device(name, host.output_devices()?, host.default_output_device(), "Output")
}

fn find_device(
    name: Option<&str>,
    devices: impl Iterator<Item = Device>,
    default: Option<Device>,
    direction: &str,
) -> Result<Device> {
    match name {
        Some(query) => {
            for device in devices {
                if let Ok(n) = device.name() {
                    if n.contains(query) {
                        return Ok(device);
                    }
                }
            }
            anyhow::bail!("{direction} device '{query}' not found")
        }
        None => default.ok_or_else(|| anyhow::anyhow!("No default {direction} device")),
    }
}

// --- Stream creation ---

pub fn start_capture(
    device: &Device,
    tx: mpsc::Sender<Vec<i16>>,
    m: Arc<PipelineMetrics>,
) -> Result<(AudioHandle, StreamConfig)> {
    let supported = device.default_input_config()?;
    let format = supported.sample_format();
    let config: StreamConfig = supported.into();

    let stream = match format {
        SampleFormat::I16 => device.build_input_stream(
            &config,
            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                m.set_input_level(metrics::compute_rms(data), metrics::compute_peak(data));
                let _ = tx.try_send(data.to_vec());
            },
            |e| eprintln!("Input error: {e}"),
            None,
        )?,
        SampleFormat::F32 => {
            let m = m.clone();
            device.build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let pcm: Vec<i16> = data.iter().map(|&s| (s * 32767.0) as i16).collect();
                    m.set_input_level(metrics::compute_rms(&pcm), metrics::compute_peak(&pcm));
                    let _ = tx.try_send(pcm);
                },
                |e| eprintln!("Input error: {e}"),
                None,
            )?
        }
        _ => anyhow::bail!("Unsupported input format: {format}"),
    };

    stream.play()?;
    Ok((AudioHandle { _stream: stream }, config))
}

pub fn start_playback(
    device: &Device,
    buffer: Arc<Mutex<VecDeque<i16>>>,
    m: Arc<PipelineMetrics>,
) -> Result<(AudioHandle, StreamConfig)> {
    let supported = device.default_output_config()?;
    let format = supported.sample_format();
    let config: StreamConfig = supported.into();

    let stream = match format {
        SampleFormat::I16 => device.build_output_stream(
            &config,
            move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                {
                    let mut buf = buffer.lock().unwrap();
                    for s in data.iter_mut() {
                        *s = buf.pop_front().unwrap_or(0);
                    }
                    m.set_buffer_depth(buf.len());
                }
                m.set_output_level(metrics::compute_rms(data), metrics::compute_peak(data));
            },
            |e| eprintln!("Output error: {e}"),
            None,
        )?,
        SampleFormat::F32 => {
            let m = m.clone();
            device.build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let mut pcm = vec![0i16; data.len()];
                    {
                        let mut buf = buffer.lock().unwrap();
                        for (i, s) in data.iter_mut().enumerate() {
                            let sample = buf.pop_front().unwrap_or(0);
                            pcm[i] = sample;
                            *s = sample as f32 / 32767.0;
                        }
                        m.set_buffer_depth(buf.len());
                    }
                    m.set_output_level(metrics::compute_rms(&pcm), metrics::compute_peak(&pcm));
                },
                |e| eprintln!("Output error: {e}"),
                None,
            )?
        }
        _ => anyhow::bail!("Unsupported output format: {format}"),
    };

    stream.play()?;
    Ok((AudioHandle { _stream: stream }, config))
}

// --- Audio encoding pipeline ---

pub async fn encode_and_send(
    mut rx: mpsc::Receiver<Vec<i16>>,
    tx: mpsc::Sender<String>,
    sample_rate: u32,
    channels: u16,
    api_rate: u32,
    m: Arc<PipelineMetrics>,
) -> Result<()> {
    while let Some(raw) = rx.recv().await {
        let mono = to_mono(&raw, channels);
        let resampled = resample(&mono, sample_rate, api_rate);

        let rms = metrics::compute_rms(&resampled);
        m.push_input_history(rms as f64);
        let bytes: Vec<u8> = resampled.iter().flat_map(|s| s.to_le_bytes()).collect();
        m.add_sent(resampled.len() as u64, bytes.len() as u64);

        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let json = serde_json::to_string(&ClientEvent::AppendAudio { audio: b64 })?;
        tx.send(json).await?;
    }
    Ok(())
}

// --- DSP helpers ---

pub fn to_mono(input: &[i16], channels: u16) -> Vec<i16> {
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

pub fn resample(input: &[i16], from: u32, to: u32) -> Vec<i16> {
    if from == to {
        return input.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let len = (input.len() as f64 / ratio) as usize;
    (0..len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let idx = pos as usize;
            let frac = pos - idx as f64;
            if idx + 1 < input.len() {
                (input[idx] as f64 + (input[idx + 1] as f64 - input[idx] as f64) * frac) as i16
            } else {
                input[idx.min(input.len() - 1)]
            }
        })
        .collect()
}

pub fn expand_channels(input: &[i16], channels: u16) -> Vec<i16> {
    if channels == 1 {
        return input.to_vec();
    }
    input
        .iter()
        .flat_map(|&s| std::iter::repeat_n(s, channels as usize))
        .collect()
}
