use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub port: u16,
    pub providers: Vec<Provider>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Provider {
    pub base_url: String,
    pub key: String,
    #[serde(default)]
    pub extra_params: HashMap<String, serde_yaml::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ModelInfo {
    pub id: String,
    pub object: Option<String>,
    pub created: Option<i64>,
    pub owned_by: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct ModelsResponse {
    pub data: Vec<ModelInfo>,
    pub object: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct ChatCompletionRequest {
    pub model: String,
    #[serde(flatten)]
    pub messages: serde_yaml::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}
