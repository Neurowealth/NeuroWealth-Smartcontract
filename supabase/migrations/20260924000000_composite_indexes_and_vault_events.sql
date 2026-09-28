-- Issue #764: composite indexes for deposits/withdrawals reconciliation
-- queries, plus the vault_events table + helper index it was missing.
--
-- vault_events is written by agent/src/eventListener.ts:logEventToDb() on
-- every processed ledger event (INSERT ... ON CONFLICT DO NOTHING keyed on
-- event_id) but was never defined in a migration, so it only existed as an
-- implicit table on whatever database the agent happened to run against.

CREATE TABLE IF NOT EXISTS vault_events (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    event_id TEXT UNIQUE NOT NULL,
    event_type TEXT NOT NULL,
    ledger_sequence BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

ALTER TABLE vault_events ENABLE ROW LEVEL SECURITY;

-- Helper index: the agent's ledger cursor replay and the event dashboard
-- both filter by event_type and page through history newest-first.
CREATE INDEX IF NOT EXISTS idx_vault_events_type_ledger
    ON vault_events(event_type, ledger_sequence DESC);

-- Composite indexes on deposits/withdrawals: reconciliation and support
-- tooling look up a specific user's transaction by tx_hash (unique lookups
-- already covered by the UNIQUE constraint) *and* separately need to page
-- a user's history filtered to a minimum amount (e.g. "large deposits" /
-- fraud-review queries), which idx_*_user_timestamp does not cover since
-- amount isn't a leading or included column.
CREATE INDEX IF NOT EXISTS idx_deposits_user_amount
    ON deposits(user_id, amount DESC);
CREATE INDEX IF NOT EXISTS idx_withdrawals_user_amount
    ON withdrawals(user_id, amount DESC);
