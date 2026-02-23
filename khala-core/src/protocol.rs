use serde::{Deserialize, Serialize};

#[derive(Serialize)]
#[serde(tag = "type")]
pub enum ClientEvent {
    #[serde(rename = "session.update")]
    SessionUpdate { session: SessionConfig },
    #[serde(rename = "input_audio_buffer.append")]
    AppendAudio { audio: String },
    #[serde(rename = "input_audio_buffer.commit")]
    CommitAudio {},
    #[serde(rename = "response.create")]
    CreateResponse { response: CreateResponseConfig },
    #[serde(rename = "response.cancel")]
    CancelResponse {},
    #[serde(rename = "conversation.item.delete")]
    DeleteItem { item_id: String },
}

#[derive(Serialize)]
pub struct SessionConfig {
    pub modalities: Vec<String>,
    pub instructions: String,
    pub voice: String,
    pub input_audio_format: String,
    pub output_audio_format: String,
    pub turn_detection: Option<TurnDetection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_audio_noise_reduction: Option<NoiseReduction>,
    pub temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_response_output_tokens: Option<u32>,
}

#[derive(Serialize)]
pub struct NoiseReduction {
    #[serde(rename = "type")]
    pub reduction_type: String,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_response: Option<bool>,
}

#[derive(Serialize)]
pub struct CreateResponseConfig {
    pub modalities: Vec<String>,
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
    #[serde(rename = "response.text.delta")]
    TextDelta { delta: String },
    #[serde(rename = "response.text.done")]
    TextDone {},
    #[serde(rename = "input_audio_buffer.speech_started")]
    SpeechStarted {},
    #[serde(rename = "input_audio_buffer.speech_stopped")]
    SpeechStopped {},
    #[serde(rename = "conversation.item.created")]
    ItemCreated { item: ConversationItem },
    #[serde(rename = "response.created")]
    ResponseCreated {},
    #[serde(rename = "response.done")]
    ResponseDone {
        #[serde(default)]
        response: Option<ResponseInfo>,
    },
    #[serde(rename = "error")]
    Error { error: ApiError },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
pub struct ConversationItem {
    pub id: String,
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Deserialize)]
pub struct ResponseInfo {
    pub status: Option<String>,
    #[serde(default)]
    pub output: Vec<OutputItem>,
}

#[derive(Deserialize)]
pub struct OutputItem {
    pub id: String,
}

#[derive(Deserialize)]
pub struct ApiError {
    pub code: Option<String>,
    pub message: Option<String>,
}
