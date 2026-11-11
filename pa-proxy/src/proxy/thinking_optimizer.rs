//! Minimal thinking helper required by the copied Codex Chat transform.
//!
//! The upstream cc-switch optimizer is Anthropic-specific. We only need the
//! "adaptive thinking is the default" predicate so the transform can decide
//! whether to inject Claude thinking options. DeepSeek never matches, so this
//! returns false and the transform leaves DeepSeek's own reasoning fields alone.

fn normalize_model_name(model: &str) -> String {
    model.trim().to_ascii_lowercase().replace(['.', '_'], "-")
}

pub(crate) fn adaptive_thinking_is_default(model: &str) -> bool {
    let normalized = normalize_model_name(model);
    ["fable-5", "mythos-5", "mythos-preview", "opus-5", "sonnet-5"]
        .iter()
        .any(|needle| normalized.contains(needle))
}
