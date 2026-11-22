-- Agent key/value state (event listener cursor, state backups; see agent/src/ledgerCursor.ts)
CREATE TABLE IF NOT EXISTS agent_state (
    key TEXT PRIMARY KEY,
    value JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

ALTER TABLE agent_state ENABLE ROW LEVEL SECURITY;

CREATE POLICY agent_state_service_role_all ON agent_state
    AS PERMISSIVE FOR ALL TO service_role USING (true) WITH CHECK (true);
