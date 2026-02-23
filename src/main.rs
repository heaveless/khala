mod cli;
mod config;
mod ui;

use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::Parser;

use khala_core::metrics::PipelineMetrics;
use khala_core::pipeline::PipelineParams;

const SOCKET_TIMEOUT: Duration = Duration::from_secs(60);
const SOCKET_POLL_INTERVAL: Duration = Duration::from_millis(500);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();

    match cli.command {
        cli::Commands::Start { rvc, no_rvc } => {
            let rvc_override = if rvc { Some(true) } else if no_rvc { Some(false) } else { None };
            cmd_start(rvc_override).await
        }
        cli::Commands::Doctor => cmd_doctor(),
        cli::Commands::Config => cmd_config(),
        cli::Commands::Logs => cmd_logs(),
    }
}

// --- Commands ---

async fn cmd_start(rvc_override: Option<bool>) -> anyhow::Result<()> {
    if !run_preflight_checks() {
        anyhow::bail!(
            "Pre-flight checks failed. Run 'khala doctor' for details."
        );
    }

    let mut cfg = config::Config::load()?;

    // CLI flags override config: --rvc forces on, --no-rvc forces off
    let rvc_wanted = rvc_override.unwrap_or(cfg.rvc.enabled);
    let rvc_active = rvc_wanted && which::which("khala-rvc").is_ok();
    if rvc_wanted && !rvc_active {
        eprintln!("RVC enabled but khala-rvc not found in PATH. Running without RVC.");
    }

    let rvc_child = if rvc_active {
        let _ = std::fs::remove_file(&cfg.rvc.socket);
        let child = start_rvc_server(&cfg.rvc, &cfg.log_dir())?;

        if let Err(e) = wait_for_socket(&cfg.rvc.socket, &cfg.log_dir()).await {
            cleanup_rvc(child, &cfg.rvc.socket);
            return Err(e);
        }

        cfg.pipeline.rvc_socket_path = Some(cfg.rvc.socket.clone());
        Some(child)
    } else {
        None
    };

    let socket = cfg.rvc.socket.clone();
    let result = run_app(Arc::new(cfg)).await;

    if let Some(child) = rvc_child {
        cleanup_rvc(child, &socket);
    }

    result
}

fn cmd_doctor() -> anyhow::Result<()> {
    println!("Khala Doctor\n");
    let all_ok = run_doctor_checks();
    println!();
    if all_ok {
        println!("All checks passed.");
    } else {
        println!("Some checks failed. Fix the issues above and run 'khala doctor' again.");
    }
    Ok(())
}

/// Runs all doctor checks, printing results. Returns true if all checks passed.
fn run_doctor_checks() -> bool {
    run_checks(true)
}

/// Silent pre-flight: only prints a summary line.
fn run_preflight_checks() -> bool {
    let ok = run_checks(false);
    if ok {
        eprintln!("All dependencies verified.");
    }
    ok
}

fn run_checks(verbose: bool) -> bool {
    let mut all_ok = true;

    // 1. Config file — load it (auto-creates if missing)
    let cfg = match config::Config::load() {
        Ok(cfg) => {
            if verbose {
                check_pass("Config", &format!("{}", config::config_path().display()));
            }
            Some(cfg)
        }
        Err(e) => {
            if verbose {
                check_fail("Config", &format!("{e}"));
            }
            all_ok = false;
            None
        }
    };

    // 2. OPENAI_API_KEY
    if let Some(cfg) = &cfg {
        if cfg.pipeline.api_key.is_empty() {
            if verbose {
                check_fail("OPENAI_API_KEY", "not set (env var or config [openai].api_key)");
            }
            all_ok = false;
        } else if verbose {
            check_pass("OPENAI_API_KEY", "set");
        }
    }

    // 3-7. RVC checks
    if let Some(cfg) = &cfg {
        if !cfg.rvc.enabled {
            // Manually disabled in config
            if verbose {
                check_skip("RVC", "disabled in config");
            }
        } else {
            match which::which("khala-rvc") {
                Ok(path) => {
                    if verbose {
                        check_pass("khala-rvc", &format!("{}", path.display()));
                    }
                    // Binary found — validate all dependencies
                    check_path(&cfg.rvc.lib, "RVC lib", true, &mut all_ok, verbose);
                    check_path(&cfg.rvc.model, "RVC model", false, &mut all_ok, verbose);
                    check_path(&cfg.rvc.index, "RVC index", false, &mut all_ok, verbose);
                    check_path(&cfg.rvc.hubert, "HuBERT", false, &mut all_ok, verbose);
                    check_path(&cfg.rvc.rmvpe, "RMVPE", false, &mut all_ok, verbose);
                }
                Err(_) => {
                    // Binary not found — auto-disable, skip dep checks
                    if verbose {
                        check_skip("RVC", "khala-rvc not found in PATH (auto-disabled)");
                    }
                }
            }
        }
    }

    all_ok
}

fn check_path(path: &std::path::Path, name: &str, check_empty: bool, all_ok: &mut bool, verbose: bool) {
    if check_empty && path.as_os_str().is_empty() {
        if verbose {
            check_fail(name, &format!("not set ([rvc].{} in config)", name.to_lowercase().replace(' ', "_")));
        }
        *all_ok = false;
    } else if path.exists() {
        if verbose {
            check_pass(name, &format!("{}", path.display()));
        }
    } else {
        if verbose {
            check_fail(name, &format!("not found: {}", path.display()));
        }
        *all_ok = false;
    }
}

fn cmd_config() -> anyhow::Result<()> {
    let path = config::config_path();

    if path.exists() {
        println!("{}", path.display());
        println!();
        let content = std::fs::read_to_string(&path)?;
        print!("{content}");
    } else {
        println!("Config not found: {}", path.display());
        println!("Run 'khala start' to auto-create with defaults.");
    }

    Ok(())
}

fn cmd_logs() -> anyhow::Result<()> {
    let data_dir = config::data_dir();
    let log_dir = data_dir.join("logs");

    if !log_dir.exists() {
        println!("No logs yet. Log directory: {}", log_dir.display());
        return Ok(());
    }

    let mut entries: Vec<_> = std::fs::read_dir(&log_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    if entries.is_empty() {
        println!("No log files in {}", log_dir.display());
        return Ok(());
    }

    for entry in &entries {
        let path = entry.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        println!("==> {} <==", name);
        match std::fs::read_to_string(&path) {
            Ok(content) if content.is_empty() => println!("(empty)"),
            Ok(content) => print!("{content}"),
            Err(e) => println!("(error reading: {e})"),
        }
        println!();
    }

    println!("Log directory: {}", log_dir.display());
    Ok(())
}

// --- Doctor helpers ---

fn check_pass(name: &str, detail: &str) {
    println!("  [ok]   {name}: {detail}");
}

fn check_fail(name: &str, detail: &str) {
    println!("  [FAIL] {name}: {detail}");
}

fn check_skip(name: &str, detail: &str) {
    println!("  [skip] {name}: {detail}");
}

// --- App runner ---

async fn run_app(cfg: Arc<config::Config>) -> anyhow::Result<()> {
    let fwd_metrics = Arc::new(PipelineMetrics::new(false));
    let rev_metrics = Arc::new(PipelineMetrics::new(true));

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

    let fwd_label = format!(
        "{} -> {}",
        cfg.pipeline.source_lang, cfg.pipeline.target_lang
    );
    let rev_label = format!(
        "{} -> {}",
        cfg.pipeline.target_lang, cfg.pipeline.source_lang
    );

    let app_cfg = cfg.clone();

    let forward = khala_core::pipeline::run(PipelineParams {
        cfg: &cfg.pipeline,
        input_device: cfg.pipeline.mic_device.as_deref(),
        output_device: Some(&cfg.pipeline.virtual_output_device),
        instruction: cfg.pipeline.forward_instruction(),
        label: fwd_label.clone(),
        metrics: fwd_metrics.clone(),
        rvc_socket: cfg.pipeline.rvc_socket_path.as_deref(),
        text_only: false,
    });

    let reverse = khala_core::pipeline::run(PipelineParams {
        cfg: &cfg.pipeline,
        input_device: Some(&cfg.pipeline.virtual_input_device),
        output_device: None,
        instruction: cfg.pipeline.reverse_instruction(),
        label: rev_label.clone(),
        metrics: rev_metrics.clone(),
        rvc_socket: None,
        text_only: true,
    });

    let app = ui::AppState {
        config: app_cfg,
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

// --- RVC lifecycle ---

fn start_rvc_server(
    rvc: &config::RvcConfig,
    log_dir: &std::path::Path,
) -> anyhow::Result<Child> {
    std::fs::create_dir_all(log_dir)?;

    eprintln!("Starting RVC server...");
    eprintln!("  Lib:    {}", rvc.lib.display());
    eprintln!("  Model:  {}", rvc.model.display());
    eprintln!("  Index:  {}", rvc.index.display());
    eprintln!("  HuBERT: {}", rvc.hubert.display());
    eprintln!("  RMVPE:  {}", rvc.rmvpe.display());
    eprintln!("  Socket: {}", rvc.socket);

    let stdout_log = std::fs::File::create(log_dir.join("rvc-stdout.log"))?;
    let stderr_log = std::fs::File::create(log_dir.join("rvc-stderr.log"))?;

    let child = Command::new("khala-rvc")
        .arg("--rvc-lib")
        .arg(&rvc.lib)
        .arg("--model")
        .arg(&rvc.model)
        .arg("--index")
        .arg(&rvc.index)
        .arg("--hubert")
        .arg(&rvc.hubert)
        .arg("--rmvpe")
        .arg(&rvc.rmvpe)
        .arg("--socket")
        .arg(&rvc.socket)
        .arg("--f0method")
        .arg(&rvc.f0method)
        .arg("--pitch")
        .arg(rvc.pitch.to_string())
        .arg("--index-rate")
        .arg(rvc.index_rate.to_string())
        .arg("--block-time")
        .arg(rvc.block_time.to_string())
        .arg("--extra-time")
        .arg(rvc.extra_time.to_string())
        .arg("--crossfade-time")
        .arg(rvc.crossfade_time.to_string())
        .env("OMP_NUM_THREADS", "1")
        .env("MKL_NUM_THREADS", "1")
        .env("PYTORCH_ENABLE_MPS_FALLBACK", "1")
        .env("PYTORCH_MPS_HIGH_WATERMARK_RATIO", "0.0")
        .stdout(Stdio::from(stdout_log))
        .stderr(Stdio::from(stderr_log))
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to start khala-rvc: {e}"))?;

    eprintln!("RVC server started (PID: {})", child.id());
    eprintln!("  Logs: {}", log_dir.display());

    Ok(child)
}

async fn wait_for_socket(path: &str, log_dir: &std::path::Path) -> anyhow::Result<()> {
    eprintln!("Waiting for RVC socket...");
    let start = Instant::now();

    loop {
        if std::path::Path::new(path).exists()
            && tokio::net::UnixStream::connect(path).await.is_ok()
        {
            eprintln!("RVC ready ({:.1}s)", start.elapsed().as_secs_f64());
            return Ok(());
        }

        if start.elapsed() > SOCKET_TIMEOUT {
            anyhow::bail!(
                "Timeout waiting for RVC socket after {:.0}s. Check {}/rvc-stderr.log",
                SOCKET_TIMEOUT.as_secs_f64(),
                log_dir.display()
            );
        }

        tokio::time::sleep(SOCKET_POLL_INTERVAL).await;
    }
}

fn cleanup_rvc(mut child: Child, socket_path: &str) {
    eprintln!("Stopping RVC server (PID: {})...", child.id());
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(socket_path);
    eprintln!("RVC server stopped.");
}
