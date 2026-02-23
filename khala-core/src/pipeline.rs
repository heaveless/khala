use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use cpal::traits::DeviceTrait;

use crate::audio;
use crate::config::PipelineConfig;
use crate::metrics::PipelineMetrics;
use crate::protocol::{ClientEvent, CreateResponseConfig, NoiseReduction, SessionConfig};
use crate::rvc::RvcClient;
use crate::websocket::{self, AudioMsg};

pub struct PipelineParams<'a> {
    pub cfg: &'a PipelineConfig,
    pub input_device: Option<&'a str>,
    pub output_device: Option<&'a str>,
    pub instruction: String,
    pub label: String,
    pub metrics: Arc<PipelineMetrics>,
    pub rvc_socket: Option<&'a str>,
    pub text_only: bool,
}

pub async fn run(params: PipelineParams<'_>) -> Result<()> {
    let PipelineParams {
        cfg, input_device, output_device, instruction, label,
        metrics, rvc_socket, text_only,
    } = params;

    metrics.set_status("Setting up...".into());

    let input = audio::find_input(input_device)?;
    metrics.push_log(format!("Input: {}", input.name().unwrap_or_default()));

    let (audio_tx, audio_rx) = mpsc::channel::<Vec<i16>>(128);
    let (_capture, capture_cfg) = audio::start_capture(&input, audio_tx, metrics.clone())?;

    let (buffer, _playback, playback_rate, playback_channels) = if text_only {
        let buffer = Arc::new(Mutex::new(VecDeque::<i16>::new()));
        metrics.push_log("Text-only mode (no audio output).".into());
        (buffer, None, 0, 0)
    } else {
        let output = audio::find_output(output_device)?;
        metrics.push_log(format!("Output: {}", output.name().unwrap_or_default()));
        let buffer = Arc::new(Mutex::new(VecDeque::<i16>::new()));
        let (playback, playback_cfg) =
            audio::start_playback(&output, buffer.clone(), metrics.clone())?;
        metrics.push_log(format!(
            "{}Hz {}ch → {}Hz {}ch",
            capture_cfg.sample_rate.0, capture_cfg.channels,
            playback_cfg.sample_rate.0, playback_cfg.channels,
        ));
        (buffer, Some(playback), playback_cfg.sample_rate.0, playback_cfg.channels)
    };

    metrics.set_status("Connecting...".into());
    let ws = websocket::connect(&cfg.api_key, &cfg.ws_url()).await?;
    let (ws_sink, ws_stream) = ws.split();

    let (msg_tx, mut msg_rx) = mpsc::channel::<String>(256);
    let session = build_session(cfg, instruction, text_only);
    msg_tx.send(serde_json::to_string(&session)?).await?;

    let api_rate = cfg.api_sample_rate;
    let capture_rate = capture_cfg.sample_rate.0;
    let capture_channels = capture_cfg.channels;

    let rvc_client = if text_only {
        None
    } else {
        match rvc_socket {
            Some(path) => match RvcClient::connect(path, api_rate, cfg.rvc_block_time).await {
                Ok(client) => {
                    metrics.push_log("RVC connected.".into());
                    Some(client)
                }
                Err(e) => {
                    metrics.push_log(format!("RVC unavailable: {e}. Passthrough mode."));
                    None
                }
            },
            None => None,
        }
    };

    let encode = audio::encode_and_send(
        audio_rx, msg_tx.clone(), capture_rate, capture_channels, api_rate, metrics.clone(),
    );

    let send_metrics = metrics.clone();
    let send = async move {
        let mut sink = ws_sink;
        while let Some(json) = msg_rx.recv().await {
            if let Err(e) = sink.send(tungstenite::Message::Text(json.into())).await {
                send_metrics.push_log(format!("Send error: {e}"));
                break;
            }
        }
    };

    let (speech_tx, speech_rx) = tokio::sync::watch::channel(false);
    let responding = Arc::new(AtomicBool::new(false));

    // Channel decouples the WebSocket receive loop from RVC processing
    // so that audio conversion never blocks message handling.
    let (out_tx, out_rx) = mpsc::channel::<AudioMsg>(256);
    let (resp_done_tx, resp_done_rx) = mpsc::channel::<()>(16);

    let recv = websocket::receive(
        ws_stream, metrics.clone(), responding.clone(), out_tx, resp_done_tx,
        msg_tx.clone(),
    );

    let audio_out = audio_output(
        out_rx, buffer, api_rate, playback_rate, playback_channels,
        rvc_client, metrics.clone(),
    );

    let min_speech_ms = cfg.min_speech_ms;
    let translate = vad_translate(
        msg_tx, speech_rx, text_only, min_speech_ms, responding, resp_done_rx,
    );

    // Client-side VAD: monitor input RMS to detect speech without server
    // truncation.  The server's turn_detection is null, so responses stream
    // uninterrupted while the user speaks the next sentence.
    let vad_metrics = metrics.clone();
    let vad_silence = Duration::from_millis(cfg.vad_silence_ms as u64);
    let client_vad = async move {
        let threshold = 0.01_f64;
        let mut speaking = false;
        let mut silence_start: Option<Instant> = None;
        let mut interval = tokio::time::interval(Duration::from_millis(10));

        loop {
            interval.tick().await;
            let rms = f32::from_bits(
                vad_metrics.input_rms.load(Ordering::Relaxed),
            ) as f64;

            if rms > threshold {
                if !speaking {
                    speaking = true;
                    let _ = speech_tx.send(true);
                    vad_metrics.set_speech_active(true);
                    vad_metrics.start_new_subtitle();
                }
                silence_start = None;
            } else if speaking {
                let start = silence_start.get_or_insert(Instant::now());
                if start.elapsed() >= vad_silence {
                    speaking = false;
                    let _ = speech_tx.send(false);
                    vad_metrics.set_speech_active(false);
                    silence_start = None;
                }
            }
        }
    };

    tokio::select! {
        r = encode    => { if let Err(e) = r { metrics.push_log(format!("Encode error: {e}")); } }
        _ = send      => { metrics.push_log("Send ended.".into()); }
        r = recv      => { if let Err(e) = r { metrics.push_log(format!("Receive error: {e}")); } }
        _ = audio_out => {}
        r = translate => { if let Err(e) = r { metrics.push_log(format!("Translate error: {e}")); } }
        _ = client_vad => {}
    }

    metrics.set_status(format!("[{label}] Stopped."));
    Ok(())
}

/// Queue-based translation: each speech pause commits audio and either starts
/// a response immediately or enqueues it.  Translations play in order without
/// cancellation so nothing is repeated or lost.  While a response streams, the
/// user keeps speaking and new sentences queue up naturally.
async fn vad_translate(
    msg_tx: mpsc::Sender<String>,
    mut speech_rx: tokio::sync::watch::Receiver<bool>,
    text_only: bool,
    min_speech_ms: u32,
    responding: Arc<AtomicBool>,
    mut resp_done_rx: mpsc::Receiver<()>,
) -> Result<()> {
    let modalities = if text_only {
        vec!["text".into()]
    } else {
        vec!["text".into(), "audio".into()]
    };

    let min_speech = Duration::from_millis(min_speech_ms as u64);
    let mut pending: u32 = 0;

    let create_response = |tx: &mpsc::Sender<String>, mods: &[String]| {
        let json = serde_json::to_string(&ClientEvent::CreateResponse {
            response: CreateResponseConfig {
                modalities: mods.to_vec(),
            },
        });
        let tx = tx.clone();
        async move {
            tx.send(json?).await.map_err(|_| anyhow::anyhow!("send channel closed"))
        }
    };

    loop {
        // Phase 1: wait for speech to start.  While idle, drain the
        // response queue so queued translations begin as soon as the
        // previous one finishes.
        loop {
            tokio::select! {
                result = speech_rx.wait_for(|&active| active) => {
                    result.map_err(|_| anyhow::anyhow!("speech channel closed"))?;
                    break;
                }
                _ = resp_done_rx.recv() => {
                    if pending > 0 {
                        pending -= 1;
                        create_response(&msg_tx, &modalities).await?;
                    }
                }
            }
        }

        let speech_start = Instant::now();

        // Phase 2: speech in progress — wait for it to end.
        speech_rx
            .wait_for(|&active| !active)
            .await
            .map_err(|_| anyhow::anyhow!("speech channel closed"))?;

        if speech_start.elapsed() < min_speech {
            continue;
        }

        // Phase 3: commit audio immediately, then create or enqueue.
        send_event(&msg_tx, &ClientEvent::CommitAudio {}).await?;

        if !responding.load(Ordering::Relaxed) {
            create_response(&msg_tx, &modalities).await?;
        } else {
            pending += 1;
        }
    }
}

async fn send_event(tx: &mpsc::Sender<String>, event: &ClientEvent) -> Result<()> {
    let json = serde_json::to_string(event)?;
    tx.send(json).await.map_err(|_| anyhow::anyhow!("send channel closed"))
}

/// Processes audio output in its own task, decoupled from the WebSocket
/// receive loop.  If an RVC client is present, audio is accumulated into
/// blocks, sent to the Python server for voice conversion, and the result
/// is written to the playback buffer.  Without RVC, samples pass through
/// directly with no accumulation delay.
async fn audio_output(
    mut rx: mpsc::Receiver<AudioMsg>,
    buffer: Arc<Mutex<VecDeque<i16>>>,
    api_rate: u32,
    playback_rate: u32,
    playback_channels: u16,
    mut rvc: Option<RvcClient>,
    metrics: Arc<PipelineMetrics>,
) {
    let write = |samples: &[i16]| {
        let resampled = audio::resample(samples, api_rate, playback_rate);
        let expanded = audio::expand_channels(&resampled, playback_channels);
        buffer.lock().unwrap().extend(expanded);
    };

    while let Some(msg) = rx.recv().await {
        match msg {
            AudioMsg::Samples(samples) => {
                let output = if let Some(client) = rvc.as_mut() {
                    if !client.is_connected() {
                        if client.try_reconnect().await {
                            metrics.push_log("RVC reconnected.".into());
                        }
                        samples
                    } else {
                        match client.process(&samples).await {
                            Ok(Some(converted)) => converted,
                            Ok(None) => continue, // accumulating
                            Err(e) => {
                                metrics.push_log(format!("RVC: {e}. Retrying in 5s..."));
                                client.disconnect();
                                samples
                            }
                        }
                    }
                } else {
                    samples
                };
                write(&output);
            }
            AudioMsg::Flush => {
                if let Some(client) = rvc.as_mut()
                    && client.is_connected()
                {
                    match client.flush().await {
                        Ok(Some(remaining)) => write(&remaining),
                        Ok(None) => {}
                        Err(_) => { client.disconnect(); }
                    }
                }
            }
            AudioMsg::Reset => {
                if let Some(client) = rvc.as_mut()
                    && client.is_connected()
                    && let Err(e) = client.reset().await
                {
                    metrics.push_log(format!("RVC reset: {e}"));
                    client.disconnect();
                }
            }
        }
    }
}

fn build_session(cfg: &PipelineConfig, instruction: String, text_only: bool) -> ClientEvent {
    let modalities = if text_only {
        vec!["text".into()]
    } else {
        vec!["text".into(), "audio".into()]
    };

    let noise_reduction = cfg.noise_reduction.as_deref().map(|t| NoiseReduction {
        reduction_type: t.to_string(),
    });

    ClientEvent::SessionUpdate {
        session: SessionConfig {
            modalities,
            instructions: instruction,
            voice: cfg.voice.clone(),
            input_audio_format: cfg.audio_format.clone(),
            output_audio_format: cfg.audio_format.clone(),
            // Disable server VAD — we use client-side VAD so the server
            // never truncates an in-flight response when the user speaks.
            turn_detection: None,
            input_audio_noise_reduction: noise_reduction,
            temperature: cfg.temperature,
            // Cap output to prevent rambling. A sentence translation rarely exceeds 100 tokens.
            max_response_output_tokens: Some(200),
        },
    }
}
