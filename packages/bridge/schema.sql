-- NeuroWealth Bridge Transfer Schema (Supabase/PostgreSQL)

CREATE TABLE bridge_transfers (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  
  -- Transfer details
  status TEXT NOT NULL DEFAULT 'pending',
  direction TEXT NOT NULL,
  source_chain TEXT NOT NULL,
  destination_chain TEXT NOT NULL,
  
  -- Addresses
  user_address TEXT NOT NULL,
  ethereum_user_address TEXT,
  stellar_user_address TEXT,
  
  -- Amounts (all in USDC base units, 6 decimals on Ethereum, 7 on Stellar)
  amount BIGINT NOT NULL,
  bridge_fee BIGINT NOT NULL,
  net_amount BIGINT NOT NULL,
  
  -- Transaction hashes
  source_chain_tx_hash TEXT,
  bridge_tx_hash TEXT,
  destination_tx_hash TEXT,

  -- Idempotency key supplied by the caller (#849). A retry after a restart
  -- resolves to the original transfer instead of creating a second one.
  idempotency_key TEXT,

  -- Confirmation-depth bookkeeping (#851) used by the reconciliation pass.
  current_confirmation_depth INTEGER,
  required_confirmation_depth INTEGER,
  
  -- Timing
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  estimated_arrival_time TIMESTAMP WITH TIME ZONE,
  
  -- Retry logic
  retries_remaining INTEGER DEFAULT 3,
  last_retry_time TIMESTAMP WITH TIME ZONE,

  -- Durable workflow state (#848): the fields a restarted bridge process reads
  -- back to resume a transfer instead of re-deriving it from process memory.
  stage TEXT NOT NULL DEFAULT 'observed',
  attempt_count INTEGER NOT NULL DEFAULT 0,
  last_error TEXT,
  next_attempt_at TIMESTAMP WITH TIME ZONE,
  last_reconciled_at TIMESTAMP WITH TIME ZONE,

  -- Error tracking
  error_message TEXT,
  
  -- Indexes
  CONSTRAINT valid_status CHECK (status IN ('pending', 'confirming', 'confirmed', 'failed', 'cancelled')),
  CONSTRAINT valid_stage CHECK (stage IN ('observed', 'submitted', 'confirmed', 'completed')),
  CONSTRAINT valid_direction CHECK (direction IN ('deposit', 'withdraw')),
  CONSTRAINT valid_source_chain CHECK (source_chain IN ('ethereum', 'stellar')),
  CONSTRAINT valid_destination_chain CHECK (destination_chain IN ('ethereum', 'stellar'))
);

-- Startup reconciliation (#850) scans everything that is not terminal. A
-- partial index keeps that pass off the terminal rows.
CREATE INDEX idx_bridge_transfers_non_terminal
  ON bridge_transfers (created_at)
  WHERE status NOT IN ('confirmed', 'cancelled');

-- Indexes for performance
CREATE INDEX idx_bridge_transfers_user ON bridge_transfers(user_address);
CREATE INDEX idx_bridge_transfers_status ON bridge_transfers(status);
CREATE INDEX idx_bridge_transfers_created_at ON bridge_transfers(created_at);
CREATE INDEX idx_bridge_transfers_source_chain_tx ON bridge_transfers(source_chain_tx_hash);
CREATE INDEX idx_bridge_transfers_bridge_tx ON bridge_transfers(bridge_tx_hash);
CREATE UNIQUE INDEX idx_bridge_transfers_idempotency
  ON bridge_transfers(idempotency_key)
  WHERE idempotency_key IS NOT NULL;

-- Audit log table
CREATE TABLE bridge_audit_log (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  transfer_id UUID REFERENCES bridge_transfers(id) ON DELETE CASCADE,
  
  event_type TEXT NOT NULL,
  details JSONB,
  
  created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
  
  CONSTRAINT valid_event_type CHECK (event_type IN (
    'transfer_initiated',
    'axelar_submitted',
    'status_updated',
    'retry_attempted',
    'transfer_confirmed',
    'transfer_failed',
    'transfer_cancelled',
    'transfer_reconciled'
  ))
);

CREATE INDEX idx_bridge_audit_transfer ON bridge_audit_log(transfer_id);
CREATE INDEX idx_bridge_audit_event_type ON bridge_audit_log(event_type);

-- Bridge statistics view
CREATE VIEW bridge_statistics AS
SELECT
  COUNT(*) as total_transfers,
  COUNT(CASE WHEN status = 'confirmed' THEN 1 END) as confirmed_transfers,
  COUNT(CASE WHEN status IN ('pending', 'confirming') THEN 1 END) as pending_transfers,
  COUNT(CASE WHEN status = 'failed' THEN 1 END) as failed_transfers,
  COUNT(CASE WHEN status = 'cancelled' THEN 1 END) as cancelled_transfers,
  
  SUM(CASE WHEN status = 'confirmed' THEN net_amount ELSE 0 END) as confirmed_volume,
  SUM(CASE WHEN status IN ('pending', 'confirming') THEN net_amount ELSE 0 END) as pending_volume,
  
  AVG(CASE WHEN status = 'confirmed' AND destination_chain_tx_hash IS NOT NULL 
      THEN EXTRACT(EPOCH FROM (updated_at - created_at))
      ELSE NULL END) as average_confirmation_time_seconds
FROM bridge_transfers
WHERE created_at > NOW() - INTERVAL '30 days';

-- Enable RLS (Row Level Security)
ALTER TABLE bridge_transfers ENABLE ROW LEVEL SECURITY;
ALTER TABLE bridge_audit_log ENABLE ROW LEVEL SECURITY;

-- Policies for Supabase (optional - adjust based on your auth model)
CREATE POLICY "Users can view own transfers" ON bridge_transfers
  FOR SELECT
  USING (user_address = current_user_id() OR current_user_role() = 'admin');

CREATE POLICY "Service role can manage all transfers" ON bridge_transfers
  FOR ALL
  USING (current_user_role() = 'service_role');
