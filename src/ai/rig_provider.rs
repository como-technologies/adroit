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

/// The context window pinned on every ollama request (`options.num_ctx`).
///
/// Ollama **silently truncates** the prompt at its default context window
/// (2048 tokens in the suite's ollama) — a corpus-bearing prompt leaves ~50
/// tokens of generation room and clips mid-fence, with no error (the
/// iteration-2 root cause behind run-1's structure retries; the assessments
/// app pinned the same value first). 8192 covers the corpus-block prompts at
/// llama3.2-class memory cost. Note the memory trade: each parallel ollama
/// runner (`OLLAMA_NUM_PARALLEL`) allocates its own KV cache scaled by
/// `num_ctx`, so wider windows multiply across parallel lanes.
pub const OLLAMA_NUM_CTX: u64 = 8192;

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
                    // Pin the context window: rig routes `additional_params`
                    // into the request's ollama `options`, and without an
                    // explicit `num_ctx` ollama silently clips the prompt at
                    // its 2048-token default (see [`OLLAMA_NUM_CTX`]). Note
                    // `req.max_tokens` is NOT enforced on this path — ollama's
                    // /api/chat has no such field (its cap is
                    // `options.num_predict`), so generation is bounded by the
                    // model/context, deliberately unclipped.
                    let agent = client
                        .agent(&self.model)
                        .preamble(&req.system)
                        .additional_params(serde_json::json!({ "num_ctx": OLLAMA_NUM_CTX }))
                        .build();
                    agent
                        .prompt(req.prompt.as_str())
                        .await
                        .map_err(|e| AiError::Api(e.to_string()))
                }
            }
        })
    }
}
