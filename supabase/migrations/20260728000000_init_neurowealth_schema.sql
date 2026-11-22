-- NeuroWealth PostgreSQL & Supabase Database Schema Migration
-- Issue #470: User positions, transaction history, and yield performance

-- Enable UUID extension
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- 1. USERS TABLE
-- Maps Stellar address & hashed phone number to user portfolio settings
CREATE TABLE IF NOT EXISTS users (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    stellar_address TEXT UNIQUE NOT NULL,
    phone_hash TEXT UNIQUE, -- Encrypted PII: SHA-256 salted hash of phone number
    strategy_preference TEXT NOT NULL DEFAULT 'balanced' CHECK (strategy_preference IN ('conservative', 'balanced', 'growth')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 2. DEPOSITS TABLE
CREATE TABLE IF NOT EXISTS deposits (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    amount NUMERIC(30, 7) NOT NULL CHECK (amount > 0),
    shares NUMERIC(30, 7) NOT NULL CHECK (shares > 0),
    tx_hash TEXT UNIQUE NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 3. WITHDRAWALS TABLE
CREATE TABLE IF NOT EXISTS withdrawals (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    amount NUMERIC(30, 7) NOT NULL CHECK (amount > 0),
    shares NUMERIC(30, 7) NOT NULL CHECK (shares > 0),
    tx_hash TEXT UNIQUE NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 4. REBALANCES TABLE
CREATE TABLE IF NOT EXISTS rebalances (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    protocol TEXT NOT NULL CHECK (protocol IN ('blend', 'dex', 'none')),
    amount_moved NUMERIC(30, 7) NOT NULL,
    apy_before NUMERIC(6, 2) NOT NULL,
    apy_after NUMERIC(6, 2) NOT NULL,
    tx_hash TEXT UNIQUE NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 5. YIELD SNAPSHOTS TABLE (For historical APY calculation and portfolio charts)
CREATE TABLE IF NOT EXISTS yield_snapshots (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    total_assets NUMERIC(30, 7) NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 6. EARNINGS HISTORY TABLE (Aggregated daily earnings per user)
CREATE TABLE IF NOT EXISTS earnings_history (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    daily_earnings NUMERIC(30, 7) NOT NULL,
    date DATE NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT unique_user_daily_earnings UNIQUE (user_id, date)
);

-- 7. AUDIT LOGS TABLE (Tracks all database write operations for security compliance)
CREATE TABLE IF NOT EXISTS audit_logs (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    table_name TEXT NOT NULL,
    action TEXT NOT NULL CHECK (action IN ('INSERT', 'UPDATE', 'DELETE')),
    record_id UUID,
    details JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- INDEXES FOR OPTIMIZED HIGH-PERFORMANCE QUERIES
CREATE INDEX IF NOT EXISTS idx_users_stellar_address ON users(stellar_address);
CREATE INDEX IF NOT EXISTS idx_users_phone_hash ON users(phone_hash);
CREATE INDEX IF NOT EXISTS idx_deposits_user_timestamp ON deposits(user_id, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_withdrawals_user_timestamp ON withdrawals(user_id, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_rebalances_timestamp ON rebalances(timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_yield_snapshots_user_timestamp ON yield_snapshots(user_id, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_earnings_history_user_date ON earnings_history(user_id, date DESC);

-- AUDIT LOG TRIGGER FUNCTION
CREATE OR REPLACE FUNCTION log_audit_trail()
RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO audit_logs (table_name, action, record_id, details)
    VALUES (
        TG_TABLE_NAME,
        TG_OP,
        COALESCE(NEW.id, OLD.id),
        row_to_json(COALESCE(NEW, OLD))
    );
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- ATTACH AUDIT TRIGGERS TO SENSITIVE TABLES
DROP TRIGGER IF EXISTS audit_users ON users;
CREATE TRIGGER audit_users AFTER INSERT OR UPDATE OR DELETE ON users FOR EACH ROW EXECUTE FUNCTION log_audit_trail();

DROP TRIGGER IF EXISTS audit_deposits ON deposits;
CREATE TRIGGER audit_deposits AFTER INSERT OR UPDATE OR DELETE ON deposits FOR EACH ROW EXECUTE FUNCTION log_audit_trail();

DROP TRIGGER IF EXISTS audit_withdrawals ON withdrawals;
CREATE TRIGGER audit_withdrawals AFTER INSERT OR UPDATE OR DELETE ON withdrawals FOR EACH ROW EXECUTE FUNCTION log_audit_trail();

-- ROW-LEVEL SECURITY (RLS) FOR SUPABASE
ALTER TABLE users ENABLE ROW LEVEL SECURITY;
ALTER TABLE deposits ENABLE ROW LEVEL SECURITY;
ALTER TABLE withdrawals ENABLE ROW LEVEL SECURITY;
ALTER TABLE yield_snapshots ENABLE ROW LEVEL SECURITY;
ALTER TABLE earnings_history ENABLE ROW LEVEL SECURITY;
ALTER TABLE rebalances ENABLE ROW LEVEL SECURITY;
ALTER TABLE audit_logs ENABLE ROW LEVEL SECURITY;

-- POLICIES: Authenticated users can read their own data
CREATE POLICY "Users can view own profile" ON users FOR SELECT USING (auth.uid() = id);
CREATE POLICY "Users can insert own profile" ON users FOR INSERT WITH CHECK (auth.uid() = id);
CREATE POLICY "Users can update own profile" ON users FOR UPDATE USING (auth.uid() = id) WITH CHECK (auth.uid() = id);
CREATE POLICY "Service role can insert users" ON users FOR INSERT WITH CHECK (auth.role() = 'service_role');
CREATE POLICY "Service role can update users" ON users FOR UPDATE USING (auth.role() = 'service_role') WITH CHECK (auth.role() = 'service_role');

CREATE POLICY "Users can view own deposits" ON deposits FOR SELECT USING (user_id = auth.uid());
CREATE POLICY "Users can view own withdrawals" ON withdrawals FOR SELECT USING (user_id = auth.uid());
CREATE POLICY "Users can view own yield snapshots" ON yield_snapshots FOR SELECT USING (user_id = auth.uid());
CREATE POLICY "Users can view own earnings history" ON earnings_history FOR SELECT USING (user_id = auth.uid());

-- Rebalances are public read for platform transparency
CREATE POLICY "Rebalances are viewable by all authenticated users" ON rebalances FOR SELECT USING (true);

-- Rebalances write policies: only agent and service role
CREATE POLICY "Agent can insert rebalances" ON rebalances FOR INSERT WITH CHECK (
    (auth.jwt() -> 'app_metadata' ->> 'role') = 'agent' OR auth.role() = 'service_role'
);
CREATE POLICY "Agent can update rebalances" ON rebalances FOR UPDATE USING (
    (auth.jwt() -> 'app_metadata' ->> 'role') = 'agent' OR auth.role() = 'service_role'
) WITH CHECK (
    (auth.jwt() -> 'app_metadata' ->> 'role') = 'agent' OR auth.role() = 'service_role'
);

CREATE POLICY "Service role can insert audit logs" ON audit_logs FOR INSERT WITH CHECK (auth.role() = 'service_role');
CREATE POLICY "Admins can view audit logs" ON audit_logs FOR SELECT USING (
    auth.role() = 'service_role'
    OR (auth.jwt() -> 'app_metadata' ->> 'role') = 'admin'
);

-- REALTIME SUBSCRIPTIONS FOR DASHBOARD UPDATES
ALTER PUBLICATION supabase_realtime ADD TABLE deposits;
ALTER PUBLICATION supabase_realtime ADD TABLE withdrawals;
ALTER PUBLICATION supabase_realtime ADD TABLE yield_snapshots;
ALTER PUBLICATION supabase_realtime ADD TABLE earnings_history;
