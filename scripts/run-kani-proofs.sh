#!/usr/bin/env bash
# Run Kani proofs for the share-math crate (Issue #672).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT/neurowealth-vault"

if ! command -v cargo-kani >/dev/null 2>&1 && ! cargo kani --version >/dev/null 2>&1; then
  echo "Kani is not installed. Install with:"
  echo "  cargo install --locked kani-verifier && cargo kani setup"
  exit 1
fi

exec cargo kani -p share-math "$@"
