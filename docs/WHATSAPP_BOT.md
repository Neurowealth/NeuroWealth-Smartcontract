# WhatsApp Bot Architecture

> **Scope:** `whatsapp/` (`neurowealth-whatsapp`), the Twilio webhook service that
> lets users use the vault over WhatsApp chat (Issues #469, #767, #771).

## Overview

The bot is a small Express service. Twilio forwards each inbound WhatsApp
message to a webhook. The bot runs a per-phone state machine (verify with OTP,
then chat), parses the message into an intent, validates it against the vault
contract's limits, and replies with TwiML in the same HTTP response. Users get a
**custodial** Stellar keypair, which the bot encrypts at rest.

```
 WhatsApp user
      │  message
      ▼
   Twilio  ── POST /api/whatsapp/webhook (form-encoded: From, Body) ──►  Express (index.ts)
                                                                          │
                                                                          ▼
                                                                   webhook.ts
      ┌───────────────────────────────────────────────────────────────────┤
      │ 1. hashPhoneNumber(From)            cryptoUtils.ts (SHA-256 + salt)│
      │ 2. checkRateLimit(phoneHash)        stateManager.ts (10 msg / min) │
      │ 3. getSession(phoneHash)            stateManager.ts (15 min idle)  │
      │ 4. parseIntent(Body)                intentParser.ts                │
      │ 5. state machine:                                                  │
      │      UNVERIFIED   → generateOTP     otpService.ts                  │
      │      AWAITING_OTP → verifyOTP → createCustodialWallet              │
      │                                     walletService.ts (AES-256-GCM) │
      │      VERIFIED     → validateIntent  contractLimits.ts              │
      │                   → getPortfolio / handleDeposit / handleWithdraw  │
      │                                     vaultRouter.ts ─► Soroban RPC  │
      └───────────────────────────────────────────────────────────────────┤
                                                                          ▼
                                                        TwiML <Message> response ──► Twilio ──► user
```

## Modules

| File | Responsibility |
|---|---|
| `src/index.ts` | Express bootstrap, `dotenv`, body parsers, `GET /health`, `POST /api/whatsapp/webhook`. |
| `src/webhook.ts` | Twilio handler and conversation state machine; builds TwiML replies. |
| `src/stateManager.ts` | In-memory sessions (`UNVERIFIED` → `AWAITING_OTP` → `VERIFIED`), 15-minute inactivity reset, fixed-window rate limit (10 messages / 60 s per phone). |
| `src/otpService.ts` | 6-digit OTP from `crypto.randomInt`, 5-minute TTL, max 3 attempts. |
| `src/otpDelivery.ts` | Sends OTPs by Twilio SMS; codes are never included in webhook TwiML. |
| `src/walletService.ts` | Creates a custodial `Keypair` per verified phone; stores only the public key and the encrypted secret. |
| `src/stateManager.ts` | Sessions (`UNVERIFIED` → `AWAITING_OTP` → `VERIFIED`), 15-minute inactivity reset, fixed-window rate limit (10 messages / 60 s per phone). Persisted in Postgres via `stateStore.ts`. |
| `src/stateStore.ts` | Durable, replica-safe storage for OTP challenges and sessions (`whatsapp_otps`, `whatsapp_sessions`); every operation is one atomic SQL statement. |
| `src/webhookIdempotency.ts` | Idempotency key derivation and the `whatsapp_inbound_webhooks` processing ledger that makes redelivered webhooks run at most once. |
| `src/keyring.ts`, `src/keyRotation.ts`, `src/rotateKeys.ts` | KEK ring (`ENCRYPTION_KEY`, `ENCRYPTION_KEY_VERSION`, `ENCRYPTION_KEY_RETIRED`) and the `npm run rotate-keys` command. |
| `src/envGuard.ts`, `src/bootstrapEnv.ts` | Boot-time rejection of missing, placeholder or insecure configuration. |
| `src/otpService.ts` | 6-digit OTP from `crypto.randomInt`, 5-minute TTL, max 3 attempts. Only a phone-bound hash is stored, and attempts are counted atomically in the database. |
| `src/walletService.ts` | Creates a custodial `Keypair` per verified phone; stores only the public key and the envelope-encrypted secret in `whatsapp_wallets`. |
| `src/cryptoUtils.ts` | Salted SHA-256 phone hashing; AES-256-GCM encrypt/decrypt of secret keys (key derived with `scrypt` from `ENCRYPTION_KEY`). |
| `src/intentParser.ts` | Keyword/regex parser → `GREETING`, `OTP_CODE`, `DEPOSIT`, `WITHDRAW`, `BALANCE`, `EARNINGS`, `STRATEGY`, `APY`, `UNKNOWN`. |
| `src/contractLimits.ts` | Mirrors the vault's deposit caps and strategy names; `validateIntent` rejects out-of-range amounts and unknown strategies before any transaction is built (#771). |
| `src/vaultRouter.ts` | Portfolio reads and deposit/withdraw calls against the vault contract. |

## Conversation flow

1. **UNVERIFIED:** any greeting or unrecognised message creates an OTP and
  sends it by SMS. The session moves to `AWAITING_OTP` only after delivery
  succeeds; the WhatsApp reply never contains the code.
2. **AWAITING_OTP:** a 6-digit code is checked by `verifyOTP`. On success the
   session becomes `VERIFIED` and a custodial wallet is created. Expired codes
   or three wrong attempts discard the OTP; the user must send `hi` again.
3. **VERIFIED:** each intent is first checked by `validateIntent`, then routed:
   - `BALANCE` / `EARNINGS` / `APY` → `getPortfolio`
   - `DEPOSIT` → `handleDeposit(amount, strategy?)`
   - `WITHDRAW` → `handleWithdraw(amount | all)`
   - `STRATEGY` → strategy confirmation (applied by the agent on its next run)
4. After 15 minutes without a message the session drops back to `UNVERIFIED`.

## Persistent, replica-safe state (#943)

Wallets, OTP challenges and sessions all live in PostgreSQL, so a restart loses
nothing and any replica can serve any message:

| Table | Holds | Notes |
|---|---|---|
| `whatsapp_wallets` | public key + AES-256-GCM encrypted secret (envelope, see below) | `phone_hash` primary key |
| `whatsapp_otps` | phone-bound SHA-256 of the code, expiry, attempt count | deleted on success, expiry or lockout |
| `whatsapp_sessions` | state, last-message time, rate-limit window and count | idle rows are purged every 10 minutes |

No table stores a phone number (only the salted `phone_hash`) or a plaintext
OTP/secret. Each operation is a single atomic statement rather than a
read-modify-write in application memory, which is what makes it multi-replica
safe: an OTP attempt is counted *before* the code is compared (parallel
guesses on different replicas cannot exceed the limit), a correct code is
consumed with `DELETE ... RETURNING` (it can be redeemed once), and the
rate-limit counter is incremented inside the database. The in-process
per-phone command lock still orders messages from one user within a replica.

Apply the migrations in `supabase/migrations/` before deploying. Disk-level
encryption of the database volume is expected in addition to the application
layer encryption of wallet secrets.

## Idempotent webhook processing

Twilio can redeliver the same inbound webhook after a timeout. Each delivery
is keyed by `twilio:inbound-message:<AccountSid>:<MessageSid>` (falling back to
the `I-Twilio-Idempotency-Token` header), and a row in
`whatsapp_inbound_webhooks` is claimed with an atomic upsert **before** the
command runs, inside the per-phone command lock.

| Delivery sees | Response | Command re-run? |
|---|---|---|
| new key | normal TwiML reply | runs once |
| `completed` | `200`, empty `<Response/>` (no second reply) | no |
| `processing`, lease live (another worker) | `409` | no, a later redelivery sees the result |
| `failed` (retryable, before side effects) | resumes as a new attempt | yes, safe |
| `processing`, lease expired, no side effects | resumes as a new attempt | yes, safe |
| `processing`, lease expired, side effects started | `200`, empty; row → `abandoned` | no, needs manual reconciliation |

The handler calls `SideEffectGuard.begin()` (persisting `side_effects_started`)
immediately before any state-changing step: OTP issue/verify, wallet creation,
deposit, withdraw, strategy change. A transient error (connection refused,
timeout, PostgreSQL class `08`, serialization failure, ...) raised before that
point marks the row `failed` and returns `503` so the next delivery retries.
Any error after it becomes the usual user-facing error reply and the row is
`completed`, because the command may already have partially applied.

`attempt_count` is a fencing token: a worker whose lease was taken over cannot
mark side effects or complete the newer attempt. Reply text is never stored
(it can contain OTP codes), so a duplicate is acknowledged without replaying
the reply. Webhooks with neither a MessageSid nor an idempotency token are
processed without deduplication and logged as a warning.

Replay behaviour is covered by `src/__tests__/webhookIdempotency.test.ts`
(sequential, concurrent and delayed duplicates; replayed greeting, OTP
submission and failed OTP guess; lease takeover and fencing).

## Intent validation against contract caps (#771)

`validateIntent` (in `src/contractLimits.ts`) runs before any vault call:

| Intent | Rule | Contract source |
|---|---|---|
| `DEPOSIT` | amount required, `> 0`, at most 7 decimals (USDC stroop precision) | USDC has 7 decimals |
| `DEPOSIT` | `>= min deposit` (default 0.1 USDC) | `MinDeposit` / `get_min_deposit` |
| `DEPOSIT` | `<= max deposit` (default 1,000 USDC) | `MaxDeposit` / `get_max_deposit` |
| `DEPOSIT` | `<= per-user cap` (default 1,000 USDC) | `UserDepositCap` / `get_user_deposit_cap` |
| `DEPOSIT` | `<= TVL cap` (default 10,000 USDC) | `TvLCap` / `get_tvl_cap` |
| `DEPOSIT`, `STRATEGY` | strategy ∈ `conservative`, `balanced`, `growth` | `set_user_strategy` → `InvalidStrategy` |
| `WITHDRAW` | `withdraw all`, or an amount `> 0` with at most 7 decimals | none |

The defaults match the contract's `initialize` values in
`neurowealth-vault/contracts/vault/src/lib.rs`. If the owner changes limits
on-chain, set the `VAULT_*` env vars below to the values returned by the
contract getters. Note that deposits no longer default to 100 USDC when the
user leaves out the amount: the bot asks for one instead.

The per-user and TVL checks look at the requested amount only. The contract
still enforces them against the user's and the vault's existing totals, so this
check catches obviously invalid requests early but does not replace on-chain
enforcement.

## Environment variables

Copy `whatsapp/.env.example` to `whatsapp/.env` (git-ignored). `dotenv` loads it
at startup.

| Variable | Required | Default | Purpose |
|---|---|---|---|
| `PORT` | no | `3000` | HTTP port for Express. |
| `ENCRYPTION_KEY` | **yes in any shared or prod environment** | insecure built-in string | Secret used to derive the AES-256-GCM key for custodial secrets. Use ≥ 32 random bytes. Rotating it makes existing encrypted secrets undecryptable. |
| `PHONE_HASH_SALT` | **yes in any shared or prod environment** | insecure built-in string | Salt for SHA-256 phone hashing. Changing it orphans every existing session and wallet. |
| `TWILIO_AUTH_TOKEN` | **yes** | — | Verifies the `X-Twilio-Signature` header on every webhook request (`src/twilioSignature.ts`). If unset, every webhook request is rejected with `500`. |
| `TWILIO_ACCOUNT_SID` | **yes for OTP delivery** | — | Twilio account used by `src/otpDelivery.ts` to send verification SMS. |
| `TWILIO_SMS_FROM` | **yes for OTP delivery** | — | Twilio phone number with SMS capability, in E.164 format. |
| `ENCRYPTION_KEY` | **yes** | — (boot fails if unset) | Current key-encryption key (KEK) that wraps the per-wallet data keys. ≥ 32 chars, `openssl rand -hex 32`. Rotate with the runbook below, never by simply replacing it. |
| `ENCRYPTION_KEY_VERSION` | no | `1` | Label stored with every wrapped data key. Bump it whenever `ENCRYPTION_KEY` changes. |
| `ENCRYPTION_KEY_RETIRED` | during a rotation | empty | `version=key,version=key` of previous KEKs, needed until `npm run rotate-keys` has finished. |
| `DATABASE_URL` | **yes** | — (boot fails if unset) | Postgres holding wallets, OTPs, sessions, delivery and webhook ledgers. The `postgres:postgres` default credentials are rejected. |
| `WHATSAPP_WEBHOOK_LEASE_MS` | no | `120000` | How long a processing attempt holds an inbound webhook before a redelivery may take it over. Keep it well above the slowest command. |
| `PHONE_HASH_SALT` | **yes** | — (boot fails if unset) | Salt for SHA-256 phone hashing, ≥ 16 chars. Changing it orphans every existing session and wallet. |
| `TWILIO_AUTH_TOKEN` | **yes** | — (boot fails if unset) | Verifies the `X-Twilio-Signature` header on every webhook request (`src/twilioSignature.ts`). |
| `TWILIO_WEBHOOK_URL` | behind a proxy/tunnel | reconstructed from the request | Public webhook URL exactly as configured in the Twilio console. Twilio signs this URL, so it must match byte for byte. |
| `TWILIO_SKIP_SIGNATURE_VALIDATION` | no | `false` | `true` disables signature checks for local testing with curl. With `NODE_ENV=production` the service refuses to boot. |
| `SOROBAN_RPC_URL` | **yes** | — (boot fails if unset) | Soroban RPC endpoint for vault calls. |
| `VAULT_CONTRACT_ID` | **yes** | — (boot fails if unset) | Vault contract address. |
| `VAULT_MIN_DEPOSIT` | no | `1000000` (0.1 USDC) | Min single deposit, **in stroops**. Mirror `get_min_deposit`. |
| `VAULT_MAX_DEPOSIT` | no | `10000000000` (1,000 USDC) | Max single deposit, in stroops. Mirror `get_max_deposit`. |
| `VAULT_USER_DEPOSIT_CAP` | no | `10000000000` (1,000 USDC) | Per-user cap, in stroops. Mirror `get_user_deposit_cap`. |
| `VAULT_TVL_CAP` | no | `100000000000` (10,000 USDC) | Vault TVL cap, in stroops. Mirror `get_tvl_cap`. |

### Boot-time environment guard (#945)

`src/bootstrapEnv.ts` is the first import of `src/index.ts` and of the
`rotate-keys` command. It loads `.env` and runs `assertEnv()` (`src/envGuard.ts`)
before any other module reads configuration. The process exits with one report
listing **every** problem when:

- `ENCRYPTION_KEY`, `PHONE_HASH_SALT`, `DATABASE_URL`, `TWILIO_AUTH_TOKEN`,
  `SOROBAN_RPC_URL` or `VAULT_CONTRACT_ID` is missing;
- a secret is too short, a known placeholder (`changeme`, `your-…`, …) or has
  fewer than 8 distinct characters;
- `DATABASE_URL` uses the default `postgres:postgres` credentials;
- `TWILIO_SKIP_SIGNATURE_VALIDATION=true` with `NODE_ENV=production`;
- the key ring settings or a `WHATSAPP_*` tuning value are malformed.

Values are never echoed. The static counterpart is
`scripts/check-no-production-defaults.py`, which CI runs to fail any PR that
reintroduces a source-level fallback (`process.env.X || 'literal'`) for these
variables or drops the guard import from an entry point.

Invalid `VAULT_*` values (non-integer, zero or negative) make `validateIntent`
throw, so the webhook replies with the generic error message rather than
silently using a wrong limit.

Replies are sent as TwiML, but OTPs are delivered separately by SMS. Configure
`TWILIO_ACCOUNT_SID`, `TWILIO_AUTH_TOKEN`, and `TWILIO_SMS_FROM` for outbound
verification messages. The SMS sender must be enabled for SMS in Twilio.

Requests without a valid `X-Twilio-Signature` are rejected with `403` before
they reach the state machine, so a caller cannot impersonate a phone number by
posting a forged `From`.

## Running locally

```bash
cd whatsapp
cp .env.example .env      # then fill in encryption, phone-hash, and Twilio SMS credentials
cp .env.example .env      # then fill in ENCRYPTION_KEY, PHONE_HASH_SALT, DATABASE_URL and TWILIO_AUTH_TOKEN
                          # (apply supabase/migrations/*.sql to that database first)
npm install
npm run dev               # ts-node src/index.ts
npm test                  # tsc + node:test (intent validation tests)
ngrok http 3000           # expose the webhook
```

In the Twilio console (Messaging → WhatsApp sandbox or sender), set **"When a
message comes in"** to `https://<ngrok-host>/api/whatsapp/webhook`, method
`POST`, and set `TWILIO_WEBHOOK_URL` to that same URL. `GET /health` returns `{"status":"ok"}` for liveness checks.

## Custodial key custody and rotation (#944)

The threat model for custodial keys is in [SECURITY.md](../SECURITY.md#whatsapp-custodial-key-custody-threat-model-issue-944).

**Envelope encryption.** Every wallet secret is encrypted (AES-256-GCM) with its
own random 32-byte data-encryption key (DEK). The DEK is wrapped by the
key-encryption key (KEK) from `ENCRYPTION_KEY`, and the wrap is bound to the
KEK's version label. `whatsapp_wallets` stores the ciphertext plus
`wrapped_dek`, `dek_iv`, `dek_tag` and `key_version`. A database dump alone
therefore never contains anything that decrypts without the KEK, and rotating
the KEK only re-wraps 32-byte DEKs; the Stellar secrets are not re-encrypted
and never leave encrypted form on disk. Wallets created before this change
(no envelope columns) still decrypt and are upgraded by the same command.

### Key rotation runbook

Routine rotation (scheduled, or after any suspected exposure of the KEK):

1. Generate a new KEK: `openssl rand -hex 32`.
2. Take a database backup (rotation rewrites `whatsapp_wallets`).
3. Update the secret store / environment of **every** replica:
   ```
   ENCRYPTION_KEY=<new key>
   ENCRYPTION_KEY_VERSION=2                 # any new label, e.g. 2026-q4
   ENCRYPTION_KEY_RETIRED=1=<previous key>  # append: 1=<k1>,2=<k2>
   ```
   Roll the replicas. New wallets are wrapped by version `2`; existing rows are
   still read through the retired key, so there is no downtime.
4. Rehearse: `npm run rotate-keys -- --dry-run` (from `whatsapp/`, after
   `npm run build`) verifies every row can be re-wrapped and writes nothing.
5. Rotate: `npm run rotate-keys`. It is idempotent and resumable, batches
   through the table, can run while the bot serves traffic (each `UPDATE` only
   applies if the row is unchanged since it was read), and prints
   `N rotated, N skipped, N failed`. Re-run if rows were skipped; investigate
   if any failed (their KEK is missing from `ENCRYPTION_KEY_RETIRED`).
6. Verify: `SELECT key_version, count(*) FROM whatsapp_wallets GROUP BY 1;`
   shows only the new version.
7. Remove the old key from `ENCRYPTION_KEY_RETIRED`, roll the replicas, and
   destroy the old key material.

Emergency rotation (KEK believed compromised): do the same, but treat every
wallet as exposed to anyone who also obtained a database copy. Re-wrapping
protects future copies only; if both the KEK and a DB snapshot leaked, the
affected users' funds must be moved to new wallets.

**Never** just replace `ENCRYPTION_KEY` without keeping the old one in
`ENCRYPTION_KEY_RETIRED`: records wrapped by the missing key cannot be
decrypted (the error names the missing version).

### Moving to a KMS/HSM

`wrapDek` / `unwrapDek` in `src/cryptoUtils.ts` are the only code that touches
KEK material. Replacing them with calls to a managed service (AWS KMS, GCP KMS,
Vault Transit, an HSM) makes the KEK non-exportable and gives per-call audit
logs and provider-side rotation; `key_version` then stores the provider's key
id/version, and `rotate-keys` keeps working unchanged. This is the recommended
next step for production holding real funds.

## Security notes and known gaps

These gaps are in the current implementation. Close them before the bot handles
real funds:

- **In-memory state.** Sessions, OTPs and custodial wallets live in process
  `Map`s. A restart loses every wallet, including the only copy of its encrypted
  secret. Persist wallets to the database (`db/` / `supabase/`) first.
- **Insecure defaults.** `ENCRYPTION_KEY` and `PHONE_HASH_SALT` fall back to
  hard-coded strings, and `scrypt` uses a fixed salt. Fail fast at startup when
  they are unset in production.
- **OTP is echoed in chat.** The welcome message includes the OTP for demo
  purposes, so it does not prove ownership of the phone. Deliver the OTP over a
  separate channel (SMS / Twilio Verify) and remove it from the reply.
- **Env-var KEK.** The KEK lives in the process environment, so anyone who can
  read a replica's environment or memory can unwrap every DEK. Use a KMS/HSM
  (see above) to close this.
- **Vault calls are simulated.** `vaultRouter.ts` returns fixed portfolio values
  and fake transaction hashes. Real integration should use
  `@neurowealth/vault-client` and sign with the decrypted custodial key. The
  key must never be logged or sent in chat.

See also [SECRETS_HYGIENE.md](SECRETS_HYGIENE.md) and the
[WhatsApp Integration](../README.md#whatsapp-integration) section of the README.
