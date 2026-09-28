-- Reconciliation ledger for outbound WhatsApp message delivery status,
-- populated from Twilio's asynchronous status callbacks (queued -> sent ->
-- delivered | failed | undelivered). Keyed by Twilio's MessageSid so
-- duplicate/out-of-order callbacks can be applied idempotently.
CREATE TABLE IF NOT EXISTS whatsapp_message_deliveries (
    message_sid TEXT PRIMARY KEY,
    phone_hash TEXT,
    -- Encrypted destination number (same AES-256-GCM scheme as
    -- whatsapp_wallets), kept only so a bounded retry can resend to the
    -- right recipient without storing the phone number in the clear.
    to_encrypted TEXT,
    to_iv TEXT,
    to_tag TEXT,
    status TEXT NOT NULL,
    error_code TEXT,
    error_reason TEXT,
    retry_count INTEGER NOT NULL DEFAULT 0,
    escalated_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_whatsapp_message_deliveries_status
    ON whatsapp_message_deliveries (status);

CREATE INDEX IF NOT EXISTS idx_whatsapp_message_deliveries_phone_hash
    ON whatsapp_message_deliveries (phone_hash);
