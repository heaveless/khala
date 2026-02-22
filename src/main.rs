mod audio;
mod config;
mod metrics;
mod pipeline;
mod protocol;
mod ui;
mod websocket;

use std::sync::Arc;
use config::Config;
use metrics::PipelineMetrics;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = Arc::new(Config::from_env()?);

    let fwd_metrics = Arc::new(PipelineMetrics::new());
    let rev_metrics = Arc::new(PipelineMetrics::new());

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

    let fwd_label = format!("{} → {}", cfg.source_lang, cfg.target_lang);
    let rev_label = format!("{} → {}", cfg.target_lang, cfg.source_lang);

    let forward = pipeline::run(
        &cfg,
        cfg.mic_device.as_deref(),
        Some(&cfg.virtual_output_device),
        cfg.forward_instruction(),
        fwd_label.clone(),
        fwd_metrics.clone(),
    );

    let reverse = pipeline::run(
        &cfg,
        Some(&cfg.virtual_input_device),
        cfg.speaker_device.as_deref(),
        cfg.reverse_instruction(),
        rev_label.clone(),
        rev_metrics.clone(),
    );

    let app = ui::AppState {
        config: cfg.clone(),
        forward: fwd_metrics,
        reverse: rev_metrics,
        forward_label: fwd_label,
        reverse_label: rev_label,
    };

    let tui = ui::run(app, shutdown_tx);

    tokio::select! {
        _ = shutdown_rx.changed() => {}
        r = forward => { if let Err(e) = r { eprintln!("Forward error: {e}"); } }
        r = reverse => { if let Err(e) = r { eprintln!("Reverse error: {e}"); } }
        r = tui     => { if let Err(e) = r { eprintln!("TUI error: {e}"); } }
    }

    Ok(())
}
