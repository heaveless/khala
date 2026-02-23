use anyhow::Result;
use base64::Engine;
use futures_util::StreamExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::metrics::{self, PipelineMetrics};
use crate::protocol::{ClientEvent, ServerEvent};

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Messages sent from the WebSocket receive loop to the audio output task.
pub enum AudioMsg {
    /// Decoded PCM16 samples from an audio delta.
    Samples(Vec<i16>),
    /// Flush any remaining buffered audio (end of response).
    Flush,
    /// Reset RVC processor state (start of new response).
    Reset,
}

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
    metrics: Arc<PipelineMetrics>,
    responding: Arc<AtomicBool>,
    audio_tx: mpsc::Sender<AudioMsg>,
    resp_done_tx: mpsc::Sender<()>,
    msg_tx: mpsc::Sender<String>,
) -> Result<()> {
    // Conversation cleanup: track item IDs so we can delete them after
    // each response, keeping the context minimal (only the system prompt).
    //
    // - `pending_items`: items committed but not yet assigned to a response.
    // - `active_items`:  items being processed by the current response.
    //
    // On ResponseCreated: pending → active (these are being translated now).
    // On ResponseDone:    delete active + response output items.
    let mut pending_items: Vec<String> = Vec::new();
    let mut active_items: Vec<String> = Vec::new();

    while let Some(msg) = stream.next().await {
        let text = match msg? {
            Message::Text(t) => t,
            Message::Close(_) => {
                metrics.push_log("Connection closed.".into());
                metrics.set_status("Disconnected".into());
                break;
            }
            _ => continue,
        };

        match serde_json::from_str::<ServerEvent>(&text) {
            Ok(ServerEvent::SessionCreated {}) => {
                metrics.push_log("Session created.".into());
                metrics.set_status("Connected".into());
            }
            Ok(ServerEvent::SessionUpdated {}) => {
                metrics.push_log("Session configured.".into());
                metrics.set_status("Listening...".into());
            }
            Ok(ServerEvent::ItemCreated { item }) => {
                // Only track user audio items. Assistant items are deleted
                // via response.output in ResponseDone.
                if item.role.as_deref() == Some("user") {
                    pending_items.push(item.id);
                }
            }
            Ok(ServerEvent::AudioDelta { delta }) => {
                let bytes = base64::engine::general_purpose::STANDARD.decode(&delta)?;
                let samples: Vec<i16> = bytes
                    .chunks_exact(2)
                    .map(|c| i16::from_le_bytes([c[0], c[1]]))
                    .collect();
                metrics.add_received(samples.len() as u64, bytes.len() as u64);
                metrics.push_output_history(metrics::compute_rms(&samples) as f64);
                let _ = audio_tx.try_send(AudioMsg::Samples(samples));
            }
            Ok(ServerEvent::AudioDone {}) => {
                for _ in 0..6 {
                    metrics.push_output_history(0.0);
                }
                let _ = audio_tx.try_send(AudioMsg::Flush);
            }
            Ok(ServerEvent::ResponseCreated {}) => {
                responding.store(true, Ordering::Relaxed);
                let _ = audio_tx.try_send(AudioMsg::Reset);
                // Snapshot: all pending items are now being translated.
                // New items committed during this response stay in pending.
                active_items.append(&mut pending_items);
            }
            Ok(ServerEvent::TextDelta { delta }) => {
                metrics.push_transcript_delta(&delta);
            }
            Ok(ServerEvent::TextDone {}) => {}
            Ok(ServerEvent::ResponseDone { response }) => {
                responding.store(false, Ordering::Relaxed);
                let completed = response
                    .as_ref()
                    .and_then(|r| r.status.as_deref())
                    == Some("completed");
                if completed && !metrics.is_speech_active() {
                    metrics.finish_transcript();
                }

                // Delete processed items to keep conversation context clean.
                // This prevents the model from re-translating old audio or
                // drifting into conversational mode as context grows.
                for id in active_items.drain(..) {
                    delete_item(&msg_tx, id);
                }
                if let Some(resp) = &response {
                    for item in &resp.output {
                        delete_item(&msg_tx, item.id.clone());
                    }
                }

                let _ = resp_done_tx.try_send(());
            }
            Ok(ServerEvent::Error { error }) => {
                let benign = matches!(
                    error.code.as_deref(),
                    Some("input_audio_buffer_commit_empty"
                        | "response_cancel_not_active"
                        | "conversation_already_has_active_response"
                        | "item_delete_invalid_item_id")
                );
                if !benign {
                    metrics.push_log(format!(
                        "API error: {:?} - {}",
                        error.code,
                        error.message.unwrap_or_default()
                    ));
                }
            }
            Ok(_) => {}
            Err(e) => metrics.push_log(format!("Parse error: {e}")),
        }
    }
    Ok(())
}

fn delete_item(tx: &mpsc::Sender<String>, item_id: String) {
    if let Ok(json) = serde_json::to_string(&ClientEvent::DeleteItem { item_id }) {
        let _ = tx.try_send(json);
    }
}
