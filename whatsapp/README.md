# NeuroWealth WhatsApp Bot Handler

WhatsApp integration layer enabling non-custodial and custodial chat-based interaction with the NeuroWealth Soroban Vault on Stellar.

> Full architecture, environment variables and known security gaps:
> [docs/WHATSAPP_BOT.md](../docs/WHATSAPP_BOT.md). Copy `.env.example` to `.env` to get started.

## Features & Architecture
- **Twilio Webhook Handler**: Receives and responds to incoming WhatsApp messages via `POST /api/whatsapp/webhook`.
- **OTP Verification Flow**: Enforces 6-digit OTP verification for new phone numbers with strict 5-minute expiry window.
- **Custodial Keypair Generation**: Creates encrypted Stellar keypairs for verified chat users using AES-256-GCM. Secret keys are encrypted at rest and never exposed in chat responses.
- **Natural Language Intent Parsing**: Parses user intents (`deposit`, `withdraw`, `balance`, `earnings`, `switch strategy`, `apy`).
- **Contract-Cap Validation**: Deposit amounts are checked against the vault's min/max deposit, per-user cap and TVL cap, and strategy names against `conservative` / `balanced` / `growth`, before any transaction is built (`src/contractLimits.ts`, `npm test`).
- **Security Protections**:
  - PII (Phone Numbers) hashed with SHA-256 at rest.
  - Per-phone rate limiting (max 10 messages/min).
  - 15-minute session timeout for inactive conversations.

## Verification Flow
1. User sends `"hi"` to WhatsApp number.
2. Bot sends 6-digit OTP code (expires in 5 minutes).
3. User enters OTP code -> Phone verified!
4. Stellar custodial wallet generated & user can check balance, deposit, or withdraw directly via chat.
