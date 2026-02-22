use anyhow::Result;
use base64::Engine;
use futures_util::StreamExt;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio_tungstenite::tungstenite::Message;

use crate::audio::{mono_to_channels, resample};
use crate::protocol::ServerEvent;

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

pub async fn connect(api_key: &str) -> Result<WsStream> {
    let url = "wss://api.openai.com/v1/realtime?model=gpt-4o-realtime-preview-2024-12-17";

    let request = http::Request::builder()
        .uri(url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("OpenAI-Beta", "realtime=v1")
        .header("Host", "api.openai.com")
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header(
            "Sec-WebSocket-Key",
            tungstenite::handshake::client::generate_key(),
        )
        .body(())?;

    let (ws_stream, _response) = tokio_tungstenite::connect_async(request).await?;
    Ok(ws_stream)
}

pub async fn recv_loop(
    mut ws_stream: futures_util::stream::SplitStream<WsStream>,
    playback_buffer: Arc<Mutex<VecDeque<i16>>>,
    output_sample_rate: u32,
    output_channels: u16,
) -> Result<()> {
    while let Some(msg) = ws_stream.next().await {
        let msg = msg?;
        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => {
                println!("WebSocket closed by server.");
                break;
            }
            _ => continue,
        };

        let event: ServerEvent = match serde_json::from_str(&text) {
            Ok(e) => e,
            Err(err) => {
                eprintln!("Failed to parse server event: {err}");
                continue;
            }
        };

        match event {
            ServerEvent::SessionCreated { .. } => {
                println!("Session created.");
            }
            ServerEvent::SessionUpdated { .. } => {
                println!("Session configured. Speak into the microphone!");
            }
            ServerEvent::ResponseAudioDelta { delta, .. } => {
                let bytes = base64::engine::general_purpose::STANDARD.decode(&delta)?;
                let samples: Vec<i16> = bytes
                    .chunks_exact(2)
                    .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
                    .collect();

                let resampled = resample(&samples, 24000, output_sample_rate);
                let expanded = mono_to_channels(&resampled, output_channels);

                let mut buf = playback_buffer.lock().unwrap();
                buf.extend(expanded);
            }
            ServerEvent::ResponseAudioDone { .. } => {}
            ServerEvent::Error { error } => {
                eprintln!(
                    "API Error: {:?} - {}",
                    error.code,
                    error.message.unwrap_or_default()
                );
            }
            _ => {}
        }
    }
    Ok(())
}
