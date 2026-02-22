mod audio;
mod protocol;
mod websocket;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use tokio::sync::mpsc;

use protocol::{ClientEvent, SessionConfig, TurnDetection};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let api_key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| anyhow::anyhow!("OPENAI_API_KEY environment variable not set"))?;

    println!("Connecting to OpenAI Realtime API...");

    let ws_stream = websocket::connect(&api_key).await?;
    let (ws_sink, ws_recv) = ws_stream.split();

    let (outgoing_tx, mut outgoing_rx) = mpsc::channel::<String>(64);

    // Send session configuration: server VAD with eager detection
    let session_update = ClientEvent::SessionUpdate {
        session: SessionConfig {
            modalities: vec!["text".into(), "audio".into()],
            instructions: "You are a voice transformation assistant. \
                Repeat back exactly what the user says, but in a different voice. \
                Do not add commentary, do not interpret, just echo the exact words \
                spoken by the user. Speak naturally and clearly."
                .into(),
            voice: "echo".into(),
            input_audio_format: "pcm16".into(),
            output_audio_format: "pcm16".into(),
            turn_detection: Some(TurnDetection {
                detection_type: "server_vad".into(),
                threshold: Some(0.5),
                silence_duration_ms: Some(300),
                prefix_padding_ms: Some(200),
            }),
        },
    };
    outgoing_tx
        .send(serde_json::to_string(&session_update)?)
        .await?;

    // Set up audio I/O
    let (mic_tx, mic_rx) = mpsc::channel::<Vec<i16>>(16);
    let playback_buffer = Arc::new(Mutex::new(VecDeque::<i16>::new()));

    let (_capture, input_config) = audio::start_capture(mic_tx)?;
    let (_playback, output_config) = audio::start_playback(playback_buffer.clone())?;

    println!(
        "Audio devices initialized.\n  Input:  {}Hz, {} ch\n  Output: {}Hz, {} ch",
        input_config.sample_rate.0,
        input_config.channels,
        output_config.sample_rate.0,
        output_config.channels
    );

    // Task: mic audio -> outgoing JSON events (streams continuously, no muting)
    let input_rate = input_config.sample_rate.0;
    let input_channels = input_config.channels;
    let outgoing_tx_mic = outgoing_tx.clone();
    let mic_task = tokio::spawn(async move {
        audio::mic_to_events(mic_rx, outgoing_tx_mic, input_rate, input_channels).await
    });

    // Task: outgoing channel -> WebSocket
    let ws_send_task = tokio::spawn(async move {
        let mut sink = ws_sink;
        use futures_util::SinkExt;
        while let Some(json) = outgoing_rx.recv().await {
            if let Err(e) = sink.send(tungstenite::Message::Text(json.into())).await {
                eprintln!("WebSocket send error: {e}");
                break;
            }
        }
    });

    // Task: WebSocket -> playback buffer
    let output_rate = output_config.sample_rate.0;
    let output_channels = output_config.channels;
    let ws_recv_task = tokio::spawn(async move {
        websocket::recv_loop(ws_recv, playback_buffer, output_rate, output_channels).await
    });

    println!("Ready! Speak into your microphone. Press Ctrl+C to exit.");

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            println!("\nShutting down...");
        }
        result = mic_task => {
            eprintln!("Mic task ended: {result:?}");
        }
        result = ws_send_task => {
            eprintln!("WS send task ended: {result:?}");
        }
        result = ws_recv_task => {
            eprintln!("WS recv task ended: {result:?}");
        }
    }

    Ok(())
}
