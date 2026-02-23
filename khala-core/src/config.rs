#[derive(Clone)]
pub struct PipelineConfig {
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
    pub min_speech_ms: u32,
    pub audio_format: String,
    pub api_sample_rate: u32,
    pub rvc_socket_path: Option<String>,
    pub rvc_block_time: f64,
    pub noise_reduction: Option<String>,
    pub temperature: f64,
    pub prompt: String,
}

impl PipelineConfig {
    pub fn ws_url(&self) -> String {
        format!("wss://api.openai.com/v1/realtime?model={}", self.model)
    }

    pub fn forward_instruction(&self) -> String {
        self.prompt
            .replace("{from}", &self.source_lang)
            .replace("{to}", &self.target_lang)
    }

    pub fn reverse_instruction(&self) -> String {
        self.prompt
            .replace("{from}", &self.target_lang)
            .replace("{to}", &self.source_lang)
    }
}
