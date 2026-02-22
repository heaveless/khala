use serde::{Deserialize, Serialize};

#[derive(Serialize)]
#[serde(tag = "type")]
pub enum ClientEvent {
    #[serde(rename = "session.update")]
    SessionUpdate { session: SessionConfig },
    #[serde(rename = "input_audio_buffer.append")]
    AppendAudio { audio: String },
}

#[derive(Serialize)]
pub struct SessionConfig {
    pub modalities: Vec<String>,
    pub instructions: String,
    pub voice: String,
    pub input_audio_format: String,
    pub output_audio_format: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_detection: Option<TurnDetection>,
}

#[derive(Serialize)]
pub struct TurnDetection {
    #[serde(rename = "type")]
    pub detection_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub silence_duration_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix_padding_ms: Option<u32>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum ServerEvent {
    #[serde(rename = "session.created")]
    SessionCreated {},
    #[serde(rename = "session.updated")]
    SessionUpdated {},
    #[serde(rename = "response.audio.delta")]
    AudioDelta { delta: String },
    #[serde(rename = "response.audio.done")]
    AudioDone {},
    #[serde(rename = "error")]
    Error { error: ApiError },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
pub struct ApiError {
    pub code: Option<String>,
    pub message: Option<String>,
}
