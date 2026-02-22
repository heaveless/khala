use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use cpal::traits::DeviceTrait;

use crate::audio;
use crate::config::Config;
use crate::metrics::PipelineMetrics;
use crate::protocol::{ClientEvent, SessionConfig, TurnDetection};
use crate::websocket;

pub async fn run(
    cfg: &Config,
    input_device: Option<&str>,
    output_device: Option<&str>,
    instruction: String,
    label: String,
    m: Arc<PipelineMetrics>,
) -> Result<()> {
    m.set_status("Setting up...".into());

    let input = audio::find_input(input_device)?;
    let output = audio::find_output(output_device)?;

    let input_name = input.name().unwrap_or_default();
    let output_name = output.name().unwrap_or_default();
    m.push_log(format!("Input: {input_name}"));
    m.push_log(format!("Output: {output_name}"));

    let (audio_tx, audio_rx) = mpsc::channel::<Vec<i16>>(16);
    let buffer = Arc::new(Mutex::new(VecDeque::<i16>::new()));

    let (_capture, cap_cfg) = audio::start_capture(&input, audio_tx, m.clone())?;
    let (_playback, play_cfg) = audio::start_playback(&output, buffer.clone(), m.clone())?;

    m.push_log(format!(
        "{}Hz {}ch → {}Hz {}ch",
        cap_cfg.sample_rate.0, cap_cfg.channels,
        play_cfg.sample_rate.0, play_cfg.channels,
    ));

    m.set_status("Connecting...".into());
    let ws = websocket::connect(&cfg.api_key, &cfg.ws_url()).await?;
    let (ws_sink, ws_stream) = ws.split();

    let (msg_tx, mut msg_rx) = mpsc::channel::<String>(64);

    let session = build_session(cfg, instruction);
    msg_tx.send(serde_json::to_string(&session)?).await?;

    let api_rate = cfg.api_sample_rate;
    let cap_rate = cap_cfg.sample_rate.0;
    let cap_ch = cap_cfg.channels;
    let play_rate = play_cfg.sample_rate.0;
    let play_ch = play_cfg.channels;

    let encode = audio::encode_and_send(audio_rx, msg_tx, cap_rate, cap_ch, api_rate, m.clone());

    let m_send = m.clone();
    let send = async move {
        let mut sink = ws_sink;
        while let Some(json) = msg_rx.recv().await {
            if let Err(e) = sink.send(tungstenite::Message::Text(json.into())).await {
                m_send.push_log(format!("Send error: {e}"));
                break;
            }
        }
    };

    let recv = websocket::receive(ws_stream, buffer, play_rate, play_ch, api_rate, m.clone());

    tokio::select! {
        r = encode => { if let Err(e) = r { m.push_log(format!("Encode error: {e}")); } }
        _ = send   => { m.push_log("Send ended.".into()); }
        r = recv   => { if let Err(e) = r { m.push_log(format!("Receive error: {e}")); } }
    }

    m.set_status(format!("[{label}] Stopped."));
    Ok(())
}

fn build_session(cfg: &Config, instruction: String) -> ClientEvent {
    ClientEvent::SessionUpdate {
        session: SessionConfig {
            modalities: vec!["text".into(), "audio".into()],
            instructions: instruction,
            voice: cfg.voice.clone(),
            input_audio_format: cfg.audio_format.clone(),
            output_audio_format: cfg.audio_format.clone(),
            turn_detection: Some(TurnDetection {
                detection_type: "server_vad".into(),
                threshold: Some(cfg.vad_threshold),
                silence_duration_ms: Some(cfg.vad_silence_ms),
                prefix_padding_ms: Some(cfg.vad_prefix_ms),
            }),
        },
    }
}
