#!/usr/bin/env bash
# Merge gates (docs/PLAN.md section 15.5). Every check must pass.
# Protected path: loop agents may not edit this file.
#
# Gates that compare with earlier code (coverage, mutation, public API) diff against
# GATES_BASE, or by default the merge base with origin/main (main if there is no remote).
# Tools a gate needs are optional locally, where a missing tool is SKIPPED, and required
# in CI (CI=true), where a missing tool fails the run. GATES_DIFF_GATES=0 skips the three
# diff gates, which do not depend on the platform; CI runs them on one platform only.
set -euo pipefail
cd "$(dirname "$0")/.."

step() { printf '\n== %s ==\n' "$1"; }

# need <command> <gate name>: true if the command exists; otherwise fails in CI, skips locally.
need() {
  if command -v "$1" >/dev/null; then
    return 0
  elif [ "${CI:-}" = "true" ]; then
    echo "$1 is required in CI for the $2 gate"; exit 1
  else
    echo "SKIPPED: $1 not installed (required in CI)"; return 1
  fi
}

gates=target/gates
mkdir -p "$gates"

if [ -z "${GATES_BASE:-}" ]; then
  upstream=main
  git rev-parse --verify --quiet origin/main >/dev/null && upstream=origin/main
  GATES_BASE="$(git merge-base HEAD "$upstream")"
fi
if [ "${GATES_DIFF_GATES:-1}" = "0" ]; then
  changed=skip
elif [ "$(git rev-parse "$GATES_BASE")" = "$(git rev-parse HEAD)" ] && git diff --quiet HEAD; then
  changed=false
else
  changed=true
fi
echo "comparing with $(git rev-parse --short "$GATES_BASE")"

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

step "determinism"
# Same results across processes (two runs), across kernels and threads (inside each run),
# and for the test suite on one thread as well as many.
probe=target/debug/determinism
"$probe" > "$gates/determinism.1"
"$probe" > "$gates/determinism.2"
if ! diff "$gates/determinism.1" "$gates/determinism.2"; then
  echo "two runs gave different results"; exit 1
fi
cat "$gates/determinism.1"
cargo test --workspace --locked -q -- --test-threads=1 > "$gates/single-thread.log" 2>&1 \
  || { cat "$gates/single-thread.log"; echo "tests fail on one thread"; exit 1; }
echo "tests pass on one thread"

step "performance"
target/debug/perf scripts/perf-budgets.txt

step "coverage on changed lines"
if [ "$changed" = skip ]; then
  echo "SKIPPED: GATES_DIFF_GATES=0"
elif [ "$changed" = false ]; then
  echo "nothing changed"
elif need cargo-llvm-cov coverage; then
  cargo llvm-cov --workspace --locked --lcov --output-path "$gates/lcov.info" -q > "$gates/coverage.log" 2>&1 \
    || { cat "$gates/coverage.log"; echo "coverage run failed"; exit 1; }
  python3 scripts/coverage_gate.py "$GATES_BASE" "$gates/lcov.info" 75
fi

step "public API matches the spec"
if [ "$changed" = skip ]; then
  echo "SKIPPED: GATES_DIFF_GATES=0"
elif [ "$changed" = false ]; then
  echo "nothing changed"
elif need cargo-public-api "public API"; then
  python3 scripts/api_gate.py "$GATES_BASE"
fi

step "mutation sample on changed code"
if [ "$changed" = skip ]; then
  echo "SKIPPED: GATES_DIFF_GATES=0"
elif [ "$changed" = false ]; then
  echo "nothing changed"
elif need cargo-mutants mutation; then
  git diff "$GATES_BASE" -- 'crates/*/src/*.rs' > "$gates/changes.diff"
  count="$(cargo mutants --in-diff "$gates/changes.diff" --list 2>/dev/null | wc -l | tr -d ' ')"
  if [ "$count" -eq 0 ]; then
    echo "no mutants in the changed code"
  else
    # At most about 40 mutants: take the first shard of a fixed split.
    shards=$(( (count + 39) / 40 ))
    echo "$count mutants; testing shard 1 of $shards"
    cargo mutants --in-diff "$gates/changes.diff" --shard "0/$shards" --timeout 300 \
      --output "$gates" > "$gates/mutants.log" 2>&1 || true
    python3 scripts/mutation_gate.py "$gates/mutants.out/outcomes.json" 0.6
  fi
fi

step "licenses and advisories"
if need cargo-deny licenses; then
  cargo deny check
fi

printf '\nall gates passed\n'
