//! Always-compiled facade for the opt-in AI layer (mirrors `forge_hook`): verb
//! handlers call [`open_provider`] with no `#[cfg]`. It returns a boxed
//! [`AiProvider`] when one is available, or `None` (the verb then prints a clear
//! "AI not available" message instead of failing deep in an adapter).

use crate::ai::{AiProvider, FakeProvider};
use crate::config::Config;

/// Resolve an AI provider, or `None` when unavailable.
///
/// Precedence:
/// 1. the `ADROIT_AI_FAKE` test seam — an offline [`FakeProvider`] echoing the
///    env value (so `new --interview` is testable with no network, no `ai`
///    feature);
/// 2. the configured rig provider, when built with `--features ai` *and*
///    `ai.enabled` is set;
/// 3. otherwise `None`.
pub fn open_provider(cfg: &Config) -> Option<Box<dyn AiProvider>> {
    if let Ok(canned) = std::env::var("ADROIT_AI_FAKE") {
        return Some(Box::new(FakeProvider { canned }));
    }
    #[cfg(feature = "ai")]
    if let Some(ai) = &cfg.ai
        && ai.enabled
    {
        match crate::ai::rig_provider::RigProvider::from_config(ai) {
            Ok(p) => return Some(Box::new(p)),
            Err(e) => {
                eprintln!("warning: AI provider unavailable: {e}");
                return None;
            }
        }
    }
    let _ = cfg;
    None
}
