#[derive(Clone)]
pub struct Config {
    pub api_key: String,
    pub model: String,
    pub source_lang: String,
    pub target_lang: String,
    pub voice: String,
    pub mic_device: Option<String>,
    pub speaker_device: Option<String>,
    pub virtual_output_device: String,
    pub virtual_input_device: String,
    pub vad_threshold: f64,
    pub vad_silence_ms: u32,
    pub vad_prefix_ms: u32,
    pub audio_format: String,
    pub api_sample_rate: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            model: "gpt-4o-realtime-preview-2024-12-17".into(),
            source_lang: "Spanish".into(),
            target_lang: "English".into(),
            voice: "echo".into(),
            mic_device: None,
            speaker_device: None,
            virtual_output_device: "BlackHole 2ch".into(),
            virtual_input_device: "BlackHole 16ch".into(),
            vad_threshold: 0.5,
            vad_silence_ms: 300,
            vad_prefix_ms: 200,
            audio_format: "pcm16".into(),
            api_sample_rate: 24000,
        }
    }
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let mut cfg = Self::default();

        cfg.api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| anyhow::anyhow!("OPENAI_API_KEY not set"))?;

        env_override("KHALA_MODEL", &mut cfg.model);
        env_override("KHALA_SOURCE_LANG", &mut cfg.source_lang);
        env_override("KHALA_TARGET_LANG", &mut cfg.target_lang);
        env_override("KHALA_VOICE", &mut cfg.voice);
        env_override("KHALA_VIRTUAL_OUT", &mut cfg.virtual_output_device);
        env_override("KHALA_VIRTUAL_IN", &mut cfg.virtual_input_device);

        if let Ok(v) = std::env::var("KHALA_MIC_DEVICE") {
            cfg.mic_device = Some(v);
        }
        if let Ok(v) = std::env::var("KHALA_SPEAKER_DEVICE") {
            cfg.speaker_device = Some(v);
        }
        if let Ok(v) = std::env::var("KHALA_VAD_THRESHOLD") {
            cfg.vad_threshold = v.parse()?;
        }
        if let Ok(v) = std::env::var("KHALA_VAD_SILENCE_MS") {
            cfg.vad_silence_ms = v.parse()?;
        }
        if let Ok(v) = std::env::var("KHALA_VAD_PREFIX_MS") {
            cfg.vad_prefix_ms = v.parse()?;
        }

        Ok(cfg)
    }

    pub fn ws_url(&self) -> String {
        format!("wss://api.openai.com/v1/realtime?model={}", self.model)
    }

    pub fn forward_instruction(&self) -> String {
        translate_prompt(&self.source_lang, &self.target_lang)
    }

    pub fn reverse_instruction(&self) -> String {
        translate_prompt(&self.target_lang, &self.source_lang)
    }

}

fn env_override(key: &str, target: &mut String) {
    if let Ok(v) = std::env::var(key) {
        *target = v;
    }
}

fn translate_prompt(from: &str, to: &str) -> String {
    format!(
        "You are a real-time voice translator. \
         Translate everything the user says from {from} to {to}. \
         Speak naturally and clearly in {to}. \
         Do not add any commentary, greetings, or extra words. \
         Just translate exactly what is said."
    )
}
