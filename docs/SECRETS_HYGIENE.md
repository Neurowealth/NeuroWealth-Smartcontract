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
| 3 | Dummy encryption keys and salts | `whatsapp/src/__tests__/cryptoUtils.test.ts` @ `c8d70b9` | **Test fixtures.** Unit test fixtures for testing encryption/decryption routines. Suppressed by fingerprint in [`.gitleaksignore`](../.gitleaksignore). | No |
| 4 | Dummy Stellar secret keys and tokens | `frontend/src/lib/errorSanitizer.test.ts`, `frontend/src/app/[locale]/error.test.tsx`, `frontend/src/components/ErrorBoundary.test.tsx` @ `4248391` | **Test fixtures.** Unit test inputs verifying redaction of sensitive data and secret keys in error handling routines (#866). Suppressed by fingerprint in [`.gitleaksignore`](../.gitleaksignore). | No |
| 5 | Dummy API key and bearer token | `packages/bridge/src/bridge-recovery.test.ts` @ `6d7f77f` | **Test fixtures.** Unit test inputs verifying error message sanitization and credential redaction (#848). Suppressed by fingerprint in [`.gitleaksignore`](../.gitleaksignore). | No |

**Conclusion:** no live project credentials were found anywhere in history.
Nothing required rotation. `scripts/deploy-devnet.sh` reads keys from the
environment at runtime and no invocation ever committed a real seed.

If a future scan *does* find a real seed: treat it as compromised
immediately (history rewrite does not un-leak it), rotate the key, move any
funds/authority off the account, and follow
[`docs/AGENT_KEY_COMPROMISE_RUNBOOK.md`](AGENT_KEY_COMPROMISE_RUNBOOK.md)
for agent keys.

## Enforcement

Three layers keep secrets out of the repository going forward:

### 1. Pre-commit hook (developer machines)

[`scripts/pre-commit-gitleaks.sh`](../scripts/pre-commit-gitleaks.sh) runs the
env-template policy check (below) and then scans **staged** changes with the
gitleaks repo ruleset, blocking the commit on any hit.
Install it once per clone:

```bash
ln -sf ../../scripts/pre-commit-gitleaks.sh .git/hooks/pre-commit
```

The hook fails closed: if `gitleaks` is not installed it refuses the commit
rather than silently skipping the scan (`brew install gitleaks` /
`go install github.com/gitleaks/gitleaks/v8@latest`).

### 2. CI gate (all pull requests and pushes)

The `secret-scan` job in
[`.github/workflows/ci.yml`](../.github/workflows/ci.yml) runs the pinned,
checksum-verified gitleaks CLI (v8.30.1 — not `gitleaks-action`, which needs
a paid license on organization repos) with full history (`fetch-depth: 0`) on every
PR targeting `main`/`develop`, every push to those branches and `feat/**`,
and the weekly scheduled run — so a leak that slips past a developer's
hooks is still caught before merge, and the whole history is re-swept
weekly as rules improve.

### 3. Env template & key-file policy (#766)

[`scripts/check-env-templates.py`](../scripts/check-env-templates.py) runs in
both the pre-commit hook (`--staged`, scanning the index) and the CI
`secret-scan` job (all tracked files). It enforces what a generic ruleset
cannot:

| Check | Fails on |
|-------|----------|
| Real env files | Any `.env`, `.env.<name>` or `<name>.env` that is not a `*.template` / `*.example` / `*.sample` |
| Key files | `*.pem`, `*.key`, `*.p12`, `*.pfx`, `*.jks`, `*.keystore`, `*.secret`, `id_rsa`/`id_ed25519`/… |
| Template values | A secret-looking variable (`*SECRET*`, `*_KEY`, `*TOKEN*`, `*PASSWORD*`, `*SALT*`, `*SEED*`, `*MNEMONIC*`, `*WEBHOOK*`) with a value that is not empty or a placeholder (`S...`, `<your-value>`, `your-…`, `${VAR}`, `changeme`, …). Public identifiers (`*_ADDRESS`, `*_ID`) are exempt |
| Template content (comments too) | Stellar secret seeds (except `SXXXX…` placeholders), PEM private keys, raw `0x` + 64-hex keys, AWS / OpenAI / Slack / Twilio tokens, and real credentials in URLs (`user:password@` placeholders are allowed) |

Run it locally with `python3 scripts/check-env-templates.py`. The same
patterns are in `.gitignore`, so such files are also hard to stage by
accident.

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
