#!/usr/bin/env bash
# Pre-commit secret scan (#605).
#
# Blocks commits that stage Stellar secret seeds (S...), .env contents, or
# anything else matched by .gitleaks.toml (which extends the gitleaks
# default ruleset).
#
# Install (one-time, per clone):
#   ln -sf ../../scripts/pre-commit-gitleaks.sh .git/hooks/pre-commit
# or, if you already use a hooks directory:
#   git config core.hooksPath <dir>   # and call this script from its pre-commit
#
# Requires gitleaks >= 8.19: https://github.com/gitleaks/gitleaks#installing

set -euo pipefail

if ! command -v gitleaks >/dev/null 2>&1; then
  echo "pre-commit: gitleaks is not installed — refusing to skip the secret scan." >&2
  echo "Install it (e.g. 'brew install gitleaks') or commit with --no-verify ONLY if you are certain no secrets are staged." >&2
  exit 1
fi

exec gitleaks git --pre-commit --staged --redact \
  --config "$(git rev-parse --show-toplevel)/.gitleaks.toml"
