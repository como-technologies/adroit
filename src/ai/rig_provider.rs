//! rig-backed [`AiProvider`] — the only `ai`-feature-gated part of the AI layer.
//!
//! rig is async; this is the single `block_on` bridge that keeps the
//! [`AiProvider`] trait (and therefore the verb handlers) synchronous. A
//! current-thread tokio runtime is held on the provider and reused per call.

use rig::client::{CompletionClient, Nothing};
use rig::completion::Prompt;
use rig::providers::{anthropic, ollama};

use crate::ai::{AiError, AiProvider, CompletionRequest};
use crate::config::{self, AiConfig, AiProviderKind};

/// A rig-backed provider. Holds the resolved model/key/host plus the runtime
/// used to drive rig's async API from the synchronous trait.
pub struct RigProvider {
    kind: AiProviderKind,
    model: String,
    key: Option<String>,
    host: Option<String>,
    runtime: tokio::runtime::Runtime,
}

impl RigProvider {
    /// Build from `ai.*` config, resolving the key for hosted providers.
    pub fn from_config(cfg: &AiConfig) -> Result<Self, AiError> {
        let key = match cfg.provider {
            AiProviderKind::Anthropic => Some(config::anthropic_key().ok_or_else(|| {
                AiError::Auth(
                    "set ADROIT_ANTHROPIC_KEY (or `adroit auth anthropic`) to use the \
                     anthropic provider"
                        .into(),
                )
            })?),
            AiProviderKind::Ollama => None,
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| AiError::Api(format!("tokio runtime: {e}")))?;
        Ok(Self {
            kind: cfg.provider,
            model: cfg.model.clone(),
            key,
            host: cfg.host.clone(),
            runtime,
        })
    }
}

impl AiProvider for RigProvider {
    fn id(&self) -> String {
        format!("{}:{}", self.kind, self.model)
    }

    fn complete(&self, req: &CompletionRequest) -> Result<String, AiError> {
        self.runtime.block_on(async {
            match self.kind {
                AiProviderKind::Anthropic => {
                    let client = anthropic::Client::builder()
                        .api_key(self.key.clone().unwrap_or_default())
                        .build()
                        .map_err(|e| AiError::Auth(e.to_string()))?;
                    let agent = client
                        .agent(&self.model)
                        .preamble(&req.system)
                        .max_tokens(u64::from(req.max_tokens))
                        .build();
                    agent
                        .prompt(req.prompt.as_str())
                        .await
                        .map_err(|e| AiError::Api(e.to_string()))
                }
                AiProviderKind::Ollama => {
                    // Default: local, no auth. A `host` override goes through the
                    // builder (still no auth — `Nothing`).
                    let client = match &self.host {
                        Some(h) => ollama::Client::builder()
                            .api_key(Nothing)
                            .base_url(h.as_str())
                            .build()
                            .map_err(|e| AiError::Api(e.to_string()))?,
                        None => {
                            ollama::Client::new(Nothing).map_err(|e| AiError::Api(e.to_string()))?
                        }
                    };
                    let agent = client.agent(&self.model).preamble(&req.system).build();
                    agent
                        .prompt(req.prompt.as_str())
                        .await
                        .map_err(|e| AiError::Api(e.to_string()))
                }
            }
        })
    }
}
