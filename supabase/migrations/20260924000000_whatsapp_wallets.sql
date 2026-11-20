CREATE TABLE IF NOT EXISTS whatsapp_wallets (
    phone_hash TEXT PRIMARY KEY,
    public_key TEXT NOT NULL,
    encrypted_data TEXT NOT NULL,
    iv TEXT NOT NULL,
    tag TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

ALTER TABLE whatsapp_wallets ENABLE ROW LEVEL SECURITY;

CREATE POLICY whatsapp_wallets_service_role_all ON whatsapp_wallets
    AS PERMISSIVE FOR ALL TO service_role USING (true) WITH CHECK (true);
