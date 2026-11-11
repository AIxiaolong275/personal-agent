#!/usr/bin/env python3
"""Add a `deepseek-flash` entry to Codex's bundled model catalog.

Codex resolves model metadata from `models-manager/models.json`. Without an
entry it falls back to `model_info_from_slug()`, which hardcodes 272k context
and emits a warning on every turn. This script writes a real entry so the
engine knows DeepSeek's actual limits.

Idempotent: re-running replaces the existing entry instead of appending.

Usage:
    python -I patches/add-deepseek-model.py [path/to/codex-rs]
"""

import json
import pathlib
import sys

SLUG = "deepseek-flash"

# DeepSeek V4.1 Flash: 1M context, 384K max output.
CONTEXT_WINDOW = 1_000_000
# Compact well before the window fills: 1M-token requests are slow and degrade
# in practice, while compaction is cheap. Tune here, not in the JSON.
AUTO_COMPACT_AT = 300_000


def build_entry(models_json: pathlib.Path) -> dict:
    prompt = (models_json.parent / "prompt.md").read_text(encoding="utf-8")
    return {
        "slug": SLUG,
        "display_name": "DeepSeek Flash",
        "description": "DeepSeek V4.1 Flash through the Personal Agent proxy.",
        # DeepSeek's chat API exposes no Codex reasoning-effort levels.
        "supported_reasoning_levels": [],
        "shell_type": "unified_exec",
        "visibility": "list",
        "supported_in_api": True,
        "priority": 1,
        "availability_nux": None,
        "upgrade": None,
        # Use Codex's generic prompt, not a GPT-specific one.
        "model_messages": {"instructions_template": prompt},
        "support_verbosity": False,
        "default_verbosity": None,
        # Freeform apply_patch is not offered on the Chat Completions wire.
        "apply_patch_tool_type": None,
        "truncation_policy": {"mode": "tokens", "limit": 10_000},
        "context_window": CONTEXT_WINDOW,
        "max_context_window": CONTEXT_WINDOW,
        "auto_compact_token_limit": AUTO_COMPACT_AT,
        "effective_context_window_percent": 95,
        "experimental_supported_tools": [],
        "input_modalities": ["text"],
        "supports_reasoning_summary_parameter": False,
        "supports_search_tool": False,
        "web_search_tool_type": "text",
        "use_responses_lite": False,
        "include_apps_usage_instructions": False,
        "include_skills_usage_instructions": False,
        "include_plugin_usage_instructions": False,
    }


def main() -> int:
    root = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "research/codex/codex-rs")
    models_json = root / "models-manager" / "models.json"
    data = json.loads(models_json.read_text(encoding="utf-8"))

    models = [m for m in data["models"] if m.get("slug") != SLUG]
    models.append(build_entry(models_json))
    data["models"] = models

    models_json.write_text(
        json.dumps(data, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )
    print(f"wrote {SLUG} into {models_json} ({len(models)} models)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
