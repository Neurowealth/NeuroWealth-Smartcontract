CREATE TABLE IF NOT EXISTS agent_rebalance_operations (
    operation_id TEXT PRIMARY KEY,
    decision_id TEXT NOT NULL,
    attempt INTEGER NOT NULL CHECK (attempt > 0),
    execution_mode TEXT NOT NULL CHECK (execution_mode IN ('live', 'dry-run')),
    status TEXT NOT NULL CHECK (
        status IN ('prepared', 'submitting', 'submitted', 'reconciling', 'confirmed', 'rejected', 'dry_run')
    ),
    tx_hash TEXT,
    input_snapshot JSONB NOT NULL,
    simulation_result JSONB,
    retry_reason TEXT,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

ALTER TABLE agent_rebalance_operations ENABLE ROW LEVEL SECURITY;

CREATE INDEX IF NOT EXISTS idx_agent_rebalance_operations_decision
    ON agent_rebalance_operations(decision_id, attempt DESC);

CREATE INDEX IF NOT EXISTS idx_agent_rebalance_operations_tx_hash
    ON agent_rebalance_operations(tx_hash) WHERE tx_hash IS NOT NULL;