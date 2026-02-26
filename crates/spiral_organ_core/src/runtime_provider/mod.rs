use reqwest::blocking::Client;
use serde_json::json;

use crate::domain::{ModelRequest, ModelResponse};
use crate::error::CoreError;
use crate::implementations::NoopProvider;
use crate::runtime_config::ProviderConfig;
use crate::traits::{CoreResult, Provider};

pub enum RuntimeProvider {
    Noop(NoopProvider),
    OpenAI(OpenAIProvider),
    Anthropic(AnthropicProvider),
}

impl RuntimeProvider {
    pub fn from_config(config: &ProviderConfig) -> Result<Self, String> {
        match config.kind.as_str() {
            "noop" => Ok(Self::Noop(NoopProvider::new(config.name.clone()))),
            "openai" | "openrouter" | "custom" => {
                let api_key = config.api_key.clone().ok_or_else(|| {
                    format!("provider api_key is required for {} kind", config.kind)
                })?;
                let default_base_url = match config.kind.as_str() {
                    "openrouter" => "https://openrouter.ai/api/v1",
                    _ => "https://api.openai.com/v1",
                };
                Ok(Self::OpenAI(OpenAIProvider {
                    name: config.name.clone(),
                    model: config.model.clone(),
                    base_url: config
                        .base_url
                        .clone()
                        .unwrap_or_else(|| default_base_url.to_string()),
                    api_key,
                    client: Client::new(),
                }))
            }
            "anthropic" => {
                let api_key = config
                    .api_key
                    .clone()
                    .ok_or_else(|| "provider api_key is required for anthropic kind".to_string())?;
                Ok(Self::Anthropic(AnthropicProvider {
                    name: config.name.clone(),
                    model: config.model.clone(),
                    base_url: config
                        .base_url
                        .clone()
                        .unwrap_or_else(|| "https://api.anthropic.com/v1".to_string()),
                    api_key,
                    client: Client::new(),
                }))
            }
            other => Err(format!("unsupported provider kind: {other}")),
        }
    }
}

impl Provider for RuntimeProvider {
    fn name(&self) -> &str {
        match self {
            Self::Noop(p) => p.name(),
            Self::OpenAI(p) => p.name(),
            Self::Anthropic(p) => p.name(),
        }
    }

    fn complete(&self, request: ModelRequest) -> CoreResult<ModelResponse> {
        match self {
            Self::Noop(p) => p.complete(request),
            Self::OpenAI(p) => p.complete(request),
            Self::Anthropic(p) => p.complete(request),
        }
    }
}

pub struct OpenAIProvider {
    name: String,
    model: String,
    base_url: String,
    api_key: String,
    client: Client,
}

impl Provider for OpenAIProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn complete(&self, request: ModelRequest) -> CoreResult<ModelResponse> {
        let endpoint = format!("{}/chat/completions", self.base_url.trim_end_matches('/'),);
        let resp = self
            .client
            .post(endpoint)
            .bearer_auth(&self.api_key)
            .json(&json!({
                "model": self.model,
                "messages": [
                    {
                        "role": "user",
                        "content": request.prompt
                    }
                ]
            }))
            .send()
            .map_err(|e| CoreError::Internal(format!("provider request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp
                .text()
                .unwrap_or_else(|_| "<failed to read error body>".to_string());
            return Err(CoreError::Internal(format!(
                "provider returned {}: {}",
                status, body
            )));
        }

        let payload: serde_json::Value = resp
            .json()
            .map_err(|e| CoreError::Internal(format!("invalid provider response json: {e}")))?;
        let output = payload
            .get("choices")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("message"))
            .and_then(|v| v.get("content"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if output.is_empty() {
            return Err(CoreError::Internal(
                "provider response has empty message content".to_string(),
            ));
        }

        Ok(ModelResponse {
            output,
            tool_calls: Vec::new(),
        })
    }
}

pub struct AnthropicProvider {
    name: String,
    model: String,
    base_url: String,
    api_key: String,
    client: Client,
}

impl Provider for AnthropicProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn complete(&self, request: ModelRequest) -> CoreResult<ModelResponse> {
        let endpoint = format!("{}/messages", self.base_url.trim_end_matches('/'));
        let resp = self
            .client
            .post(endpoint)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&json!({
                "model": self.model,
                "max_tokens": 2048,
                "messages": [
                    {
                        "role": "user",
                        "content": request.prompt
                    }
                ]
            }))
            .send()
            .map_err(|e| CoreError::Internal(format!("provider request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp
                .text()
                .unwrap_or_else(|_| "<failed to read error body>".to_string());
            return Err(CoreError::Internal(format!(
                "provider returned {}: {}",
                status, body
            )));
        }

        let payload: serde_json::Value = resp
            .json()
            .map_err(|e| CoreError::Internal(format!("invalid provider response json: {e}")))?;
        let output = payload
            .get("content")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("text"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if output.is_empty() {
            return Err(CoreError::Internal(
                "provider response has empty message content".to_string(),
            ));
        }

        Ok(ModelResponse {
            output,
            tool_calls: Vec::new(),
        })
    }
}
