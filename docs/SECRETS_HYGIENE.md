# Secrets Hygiene (#605)

This document records the full-history secret scan of this repository, the
triage of its findings, and the enforcement now in place to prevent future
leaks of Stellar secret seeds (`S...`) and `.env` contents.

## Full-history scan

- **Tool:** [gitleaks](https://github.com/gitleaks/gitleaks) v8.30.1, default
  ruleset plus the repo config in [`.gitleaks.toml`](../.gitleaks.toml)
  (adds a `stellar-secret-key` rule for `S` + 55 base32 characters).
- **Scope:** entire git history (260 commits, ~174 MB scanned), all branches
  reachable from `main`.
- **Command:** `gitleaks git --redact -v .`

### Findings and triage

| # | Finding | Location | Triage | Rotation needed |
|---|---------|----------|--------|-----------------|
| 1 | `generic-api-key`: `ETHEREUM_USDC_TOKEN_ADDRESS` | `packages/bridge/.env.example` @ `9e8ebfd` | **False positive.** The value is the *public* Ethereum mainnet USDC token contract address, not a credential. Suppressed by fingerprint in [`.gitleaksignore`](../.gitleaksignore). | No |
| 2 | Stellar-format seeds (e.g. `SDAKFNYE…`, `SOWQ3GLR…AAAA…`) | vendored `packages/vault-client/node_modules/@stellar/stellar-base/**` in historical commits `a3e0617` / `147069a` | **Not project secrets.** These are the Stellar SDK's own published documentation/test example seeds inside an accidentally committed `node_modules` tree (since removed from the working tree). They are public upstream. The `stellar-secret-key` rule allowlists `node_modules/` paths. | No |

**Conclusion:** no live project credentials were found anywhere in history.
Nothing required rotation. `scripts/deploy-devnet.sh` reads keys from the
environment at runtime and no invocation ever committed a real seed.

If a future scan *does* find a real seed: treat it as compromised
immediately (history rewrite does not un-leak it), rotate the key, move any
funds/authority off the account, and follow
[`docs/AGENT_KEY_COMPROMISE_RUNBOOK.md`](AGENT_KEY_COMPROMISE_RUNBOOK.md)
for agent keys.

## Enforcement

Two independent layers keep secrets out of the repository going forward:

### 1. Pre-commit hook (developer machines)

[`scripts/pre-commit-gitleaks.sh`](../scripts/pre-commit-gitleaks.sh) scans
**staged** changes with the repo ruleset and blocks the commit on any hit.
Install it once per clone:

```bash
ln -sf ../../scripts/pre-commit-gitleaks.sh .git/hooks/pre-commit
```

The hook fails closed: if `gitleaks` is not installed it refuses the commit
rather than silently skipping the scan (`brew install gitleaks` /
`go install github.com/gitleaks/gitleaks/v8@latest`).

### 2. CI gate (all pull requests and pushes)

The `secret-scan` job in
[`.github/workflows/ci.yml`](../.github/workflows/ci.yml) runs
`gitleaks/gitleaks-action@v2` with full history (`fetch-depth: 0`) on every
PR targeting `main`/`develop`, every push to those branches and `feat/**`,
and the weekly scheduled run — so a leak that slips past a developer's
hooks is still caught before merge, and the whole history is re-swept
weekly as rules improve.

## Rules of thumb

- Never commit `.env` files; commit `*.env.template` / `.env.example` files
  containing placeholders only.
- Stellar secret seeds belong in environment variables or a secrets
  manager, never in scripts, fixtures, or docs — use the SDK's published
  example keys if documentation needs one.
- Do not commit `node_modules/` (it is what dragged third-party example
  seeds into this repo's history).
- Suppress a false positive by adding its *fingerprint* to
  `.gitleaksignore` with a comment justifying it — never by weakening a
  rule.
