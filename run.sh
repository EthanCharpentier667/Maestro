#!/usr/bin/env bash
# Build (release) and start Maestro on http://127.0.0.1:7878
set -euo pipefail
cd "$(dirname "$0")"
command -v cargo >/dev/null || { echo "Rust is required: https://rustup.rs"; exit 1; }
command -v claude >/dev/null || echo "⚠  Claude Code CLI not found: npm i -g @anthropic-ai/claude-code"
cargo build --release
exec ./target/release/maestro "$@"
