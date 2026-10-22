-- Idempotency ledger for inbound Twilio WhatsApp webhooks. Twilio can
-- redeliver the same inbound message after a timeout; a row is claimed
-- (status = 'processing') before the command runs so redeliveries never
-- repeat state-changing commands or send a second reply.
--
--   processing - an attempt holds the lease until lease_expires_at
--   completed  - finished; duplicates are acknowledged without re-running
--   failed     - retryable failure before any side effect; safe to resume
--   abandoned  - lease expired after side effects started; outcome unknown,
--                needs manual reconciliation and is never re-run
--
-- attempt_count doubles as a fencing token so a worker whose lease was taken
-- over cannot overwrite a newer attempt's result. No message content or
-- reply text is stored (replies can contain OTP codes).
CREATE TABLE IF NOT EXISTS whatsapp_inbound_webhooks (
    idempotency_key TEXT PRIMARY KEY,
    message_sid TEXT,
    phone_hash TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('processing', 'completed', 'failed', 'abandoned')),
    attempt_count INTEGER NOT NULL DEFAULT 1,
    side_effects_started BOOLEAN NOT NULL DEFAULT FALSE,
    lease_expires_at TIMESTAMPTZ NOT NULL,
    response_status INTEGER,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ
);

-- Supports alerting on stuck/abandoned deliveries and retention sweeps.
CREATE INDEX IF NOT EXISTS idx_whatsapp_inbound_webhooks_status_updated
    ON whatsapp_inbound_webhooks (status, updated_at);
