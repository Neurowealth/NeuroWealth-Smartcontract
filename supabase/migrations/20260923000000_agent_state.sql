-- Agent key/value state (event listener cursor, state backups; see agent/src/ledgerCursor.ts)
CREATE TABLE IF NOT EXISTS agent_state (
    key TEXT PRIMARY KEY,
    value JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
