use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

use khala_core::config::PipelineConfig;

pub struct Config {
    pub model: String,
    pub voice: String,
    pub data_dir: PathBuf,
    pub rvc: RvcConfig,
    pub pipeline: PipelineConfig,
}

pub struct RvcConfig {
    pub enabled: bool,
    pub lib: PathBuf,
    pub model: PathBuf,
    pub index: PathBuf,
    pub hubert: PathBuf,
    pub rmvpe: PathBuf,
    pub socket: String,
    pub f0method: String,
    pub pitch: i32,
    pub index_rate: f64,
    pub block_time: f64,
    pub extra_time: f64,
    pub crossfade_time: f64,
}

// --- TOML deserialization (all sections and fields required) ---

#[derive(Deserialize)]
struct TomlConfig {
    openai: OpenaiSection,
    translation: TranslationSection,
    audio: AudioSection,
    devices: DevicesSection,
    vad: VadSection,
    rvc: RvcSection,
}

#[derive(Deserialize)]
struct OpenaiSection {
    api_key: Option<String>,
    model: String,
    voice: String,
    #[serde(default = "default_temperature")]
    temperature: f64,
}

#[derive(Deserialize)]
struct TranslationSection {
    source_lang: String,
    target_lang: String,
}

#[derive(Deserialize)]
struct AudioSection {
    mic_device: Option<String>,
    speaker_device: Option<String>,
    format: String,
    sample_rate: u32,
    #[serde(default)]
    noise_reduction: Option<String>,
}

#[derive(Deserialize)]
struct DevicesSection {
    virtual_output: String,
    virtual_input: String,
}

#[derive(Deserialize)]
struct VadSection {
    threshold: f64,
    silence_ms: u32,
    prefix_ms: u32,
    min_speech_ms: u32,
}

#[derive(Deserialize)]
struct RvcSection {
    enabled: bool,
    lib: String,
    model: String,
    index: String,
    hubert: String,
    rmvpe: String,
    socket: String,
    f0method: String,
    pitch: i32,
    index_rate: f64,
    block_time: f64,
    extra_time: f64,
    crossfade_time: f64,
}

fn default_temperature() -> f64 { 0.4 }

// --- Loading ---

impl Config {
    pub fn load() -> Result<Self> {
        let config_path = config_path();
        let data_dir = data_dir();

        std::fs::create_dir_all(&data_dir)
            .with_context(|| format!("Failed to create {}", data_dir.display()))?;

        let toml_cfg = load_or_create_default(&config_path, &data_dir)?;
        let prompt = load_or_create_prompt(&prompt_path())?;

        let api_key = std::env::var("OPENAI_API_KEY")
            .ok()
            .or(toml_cfg.openai.api_key)
            .context(format!(
                "OPENAI_API_KEY not set (env var or [openai].api_key in {})",
                config_path.display()
            ))?;

        Ok(Config {
            model: toml_cfg.openai.model.clone(),
            voice: toml_cfg.openai.voice.clone(),
            data_dir,
            pipeline: PipelineConfig {
                api_key,
                model: toml_cfg.openai.model,
                source_lang: toml_cfg.translation.source_lang,
                target_lang: toml_cfg.translation.target_lang,
                voice: toml_cfg.openai.voice,
                mic_device: toml_cfg.audio.mic_device,
                speaker_device: toml_cfg.audio.speaker_device,
                virtual_output_device: toml_cfg.devices.virtual_output,
                virtual_input_device: toml_cfg.devices.virtual_input,
                vad_threshold: toml_cfg.vad.threshold,
                vad_silence_ms: toml_cfg.vad.silence_ms,
                vad_prefix_ms: toml_cfg.vad.prefix_ms,
                min_speech_ms: toml_cfg.vad.min_speech_ms,
                audio_format: toml_cfg.audio.format,
                api_sample_rate: toml_cfg.audio.sample_rate,
                rvc_socket_path: None,
                rvc_block_time: toml_cfg.rvc.block_time,
                noise_reduction: toml_cfg.audio.noise_reduction,
                temperature: toml_cfg.openai.temperature,
                prompt,
            },
            rvc: RvcConfig {
                enabled: toml_cfg.rvc.enabled,
                lib: PathBuf::from(&toml_cfg.rvc.lib),
                model: PathBuf::from(&toml_cfg.rvc.model),
                index: PathBuf::from(&toml_cfg.rvc.index),
                hubert: PathBuf::from(&toml_cfg.rvc.hubert),
                rmvpe: PathBuf::from(&toml_cfg.rvc.rmvpe),
                socket: toml_cfg.rvc.socket,
                f0method: toml_cfg.rvc.f0method,
                pitch: toml_cfg.rvc.pitch,
                index_rate: toml_cfg.rvc.index_rate,
                block_time: toml_cfg.rvc.block_time,
                extra_time: toml_cfg.rvc.extra_time,
                crossfade_time: toml_cfg.rvc.crossfade_time,
            },
        })
    }

    pub fn log_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn prompt_path() -> PathBuf {
    config_dir().join("prompt.txt")
}

fn config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("khala")
}

pub fn data_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".khala")
}

const DEFAULT_CONFIG: &str = include_str!("../khala-config/config.toml");
const DEFAULT_PROMPT: &str = include_str!("../khala-config/prompt.txt");

fn load_or_create_prompt(path: &std::path::Path) -> Result<String> {
    if path.exists() {
        std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        std::fs::write(path, DEFAULT_PROMPT)
            .with_context(|| format!("Failed to write {}", path.display()))?;
        eprintln!("Created default prompt: {}", path.display());
        Ok(DEFAULT_PROMPT.to_string())
    }
}

fn load_or_create_default(path: &std::path::Path, data_dir: &std::path::Path) -> Result<TomlConfig> {
    if path.exists() {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        toml::from_str(&content)
            .with_context(|| format!("Failed to parse {}", path.display()))
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        let config_dir = path.parent().unwrap_or(std::path::Path::new("."));
        let toml_content = DEFAULT_CONFIG
            .replace("{data_dir}", &data_dir.to_string_lossy())
            .replace("{config_dir}", &config_dir.to_string_lossy());
        std::fs::write(path, &toml_content)
            .with_context(|| format!("Failed to write {}", path.display()))?;
        eprintln!("Created default config: {}", path.display());
        toml::from_str(&toml_content)
            .context("Failed to parse generated config")
    }
}
