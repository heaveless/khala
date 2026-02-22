use anyhow::Result;
use base64::Engine;
use futures_util::StreamExt;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio_tungstenite::tungstenite::Message;

use crate::audio;
use crate::metrics::{self, PipelineMetrics};
use crate::protocol::ServerEvent;

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

pub async fn connect(api_key: &str, url: &str) -> Result<WsStream> {
    let req = http::Request::builder()
        .uri(url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("OpenAI-Beta", "realtime=v1")
        .header("Host", "api.openai.com")
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", tungstenite::handshake::client::generate_key())
        .body(())?;

    let (stream, _) = tokio_tungstenite::connect_async(req).await?;
    Ok(stream)
}

pub async fn receive(
    mut stream: futures_util::stream::SplitStream<WsStream>,
    buffer: Arc<Mutex<VecDeque<i16>>>,
    sample_rate: u32,
    channels: u16,
    api_rate: u32,
    m: Arc<PipelineMetrics>,
) -> Result<()> {
    while let Some(msg) = stream.next().await {
        let text = match msg? {
            Message::Text(t) => t,
            Message::Close(_) => {
                m.push_log("Connection closed.".into());
                m.set_status("Disconnected".into());
                break;
            }
            _ => continue,
        };

        match serde_json::from_str::<ServerEvent>(&text) {
            Ok(ServerEvent::SessionCreated {}) => {
                m.push_log("Session created.".into());
                m.set_status("Connected".into());
            }
            Ok(ServerEvent::SessionUpdated {}) => {
                m.push_log("Session configured.".into());
                m.set_status("Listening...".into());
            }
            Ok(ServerEvent::AudioDelta { delta }) => {
                decode_audio(&delta, &buffer, api_rate, sample_rate, channels, &m)?;
            }
            Ok(ServerEvent::Error { error }) => {
                let msg = format!(
                    "API error: {:?} - {}",
                    error.code,
                    error.message.unwrap_or_default()
                );
                m.push_log(msg);
            }
            Ok(_) => {}
            Err(e) => m.push_log(format!("Parse error: {e}")),
        }
    }
    Ok(())
}

fn decode_audio(
    b64: &str,
    buffer: &Arc<Mutex<VecDeque<i16>>>,
    from_rate: u32,
    to_rate: u32,
    channels: u16,
    m: &Arc<PipelineMetrics>,
) -> Result<()> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64)?;
    let samples: Vec<i16> = bytes
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();

    m.add_received(samples.len() as u64, bytes.len() as u64);
    let rms = metrics::compute_rms(&samples);
    m.push_output_history(rms as f64);

    let resampled = audio::resample(&samples, from_rate, to_rate);
    let expanded = audio::expand_channels(&resampled, channels);

    buffer.lock().unwrap().extend(expanded);
    Ok(())
}
