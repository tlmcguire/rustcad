#!/usr/bin/env bash
# Merge gates. Every check must pass. Protected path: loop agents may not edit this file.
# Gates not built yet are listed in ROADMAP.md, Phase 0.
set -euo pipefail
cd "$(dirname "$0")/.."

step() { printf '\n== %s ==\n' "$1"; }

step "format";  cargo fmt --all -- --check
step "build";   cargo build --workspace --all-targets --locked
step "test";    cargo test --workspace --locked
step "clippy";  cargo clippy --workspace --all-targets --locked -- -D warnings
step "layers";  python3 scripts/check_layers.py

step "unsafe outside allowed crates"
if grep -rnE '\bunsafe\b' crates --include='*.rs' \
     | grep -v '^crates/cad-kernel-occt/' | grep -vE '^\S+:\s*//'; then
  echo "unsafe found outside cad-kernel-occt"; exit 1
fi
echo "none"

step "licenses and advisories"
if command -v cargo-deny >/dev/null; then
  cargo deny check
elif [ "${CI:-}" = "true" ]; then
  echo "cargo-deny is required in CI"; exit 1
else
  echo "SKIPPED: cargo-deny not installed (required in CI)"
fi

printf '\nall gates passed\n'
