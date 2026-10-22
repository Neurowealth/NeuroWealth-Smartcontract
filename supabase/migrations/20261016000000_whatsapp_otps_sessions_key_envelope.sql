-- Durable WhatsApp bot state (#943) and envelope-encryption metadata for
-- custodial wallet secrets (#944).

-- OTP challenges. Only a hash of the code is stored (bound to the phone hash),
-- and rows are deleted on success, expiry or attempt exhaustion. Timestamps are
-- epoch milliseconds supplied by the bot so every replica applies the same
-- expiry rules.
CREATE TABLE IF NOT EXISTS whatsapp_otps (
    phone_hash TEXT PRIMARY KEY,
    code_hash TEXT NOT NULL,
    expires_at_ms BIGINT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_whatsapp_otps_expires_at_ms
    ON whatsapp_otps (expires_at_ms);

-- Conversation sessions (state machine position + per-phone rate-limit window).
-- Holds no secrets; the phone number is only ever present as a salted hash.
CREATE TABLE IF NOT EXISTS whatsapp_sessions (
    phone_hash TEXT PRIMARY KEY,
    state TEXT NOT NULL CHECK (state IN ('UNVERIFIED', 'AWAITING_OTP', 'VERIFIED')),
    last_message_ms BIGINT NOT NULL,
    message_count INTEGER NOT NULL DEFAULT 0,
    rate_limit_window_start_ms BIGINT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_whatsapp_sessions_last_message_ms
    ON whatsapp_sessions (last_message_ms);

-- Envelope encryption: each wallet secret (encrypted_data/iv/tag) is encrypted
-- with its own random data key; the data key is wrapped by a key-encryption
-- key identified by key_version. Rotating the KEK only rewrites these columns
-- (npm run rotate-keys). Rows written before this migration keep NULLs here
-- and are upgraded by the same command.
ALTER TABLE whatsapp_wallets
    ADD COLUMN IF NOT EXISTS wrapped_dek TEXT,
    ADD COLUMN IF NOT EXISTS dek_iv TEXT,
    ADD COLUMN IF NOT EXISTS dek_tag TEXT,
    ADD COLUMN IF NOT EXISTS key_version TEXT;

CREATE INDEX IF NOT EXISTS idx_whatsapp_wallets_key_version
    ON whatsapp_wallets (key_version);
