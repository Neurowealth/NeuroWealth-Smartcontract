# NeuroWealth PostgreSQL & Supabase Database Schema

PostgreSQL / Supabase schema for tracking user positions, transaction history, yield performance, and audit trails.

## Single Source of Truth: Migrations

**All database schema changes must be made as Supabase migrations in `supabase/migrations/`.**

The schema is maintained through versioned migration files:
- `20260728000000_init_neurowealth_schema.sql` — Core tables, indexes, RLS policies, and audit triggers
- `20260923000000_agent_state.sql` — Agent key/value state table
- `20260924000000_whatsapp_wallets.sql` — WhatsApp wallet storage

### Creating New Migrations

To add or modify schema:

```bash
# Create a new migration file
supabase migration new your_migration_name

# Or manually create with timestamp
touch supabase/migrations/$(date -u +%Y%m%d%H%M%S)_your_migration_name.sql
```

### Applying Migrations

**Supabase (managed):**
Migrations in `supabase/migrations/` are automatically applied when you push or deploy.

**Standalone PostgreSQL:**
```bash
# Apply all migrations in order
for migration in supabase/migrations/*.sql; do
  psql $DATABASE_URL -f "$migration"
done
```

### Schema Deprecation Notice

`db/schema.sql` is **deprecated** and kept only for reference. It will be removed in a future release.
Do not edit `db/schema.sql` — all changes must be made as new migrations.

## Database Tables
1. `users`: Stores user Stellar address, hashed phone number (PII encrypted), strategy preference, and creation timestamp.
2. `deposits`: Logs deposit transactions, share minting, and transaction hashes.
3. `withdrawals`: Logs withdrawal transactions and share burning.
4. `rebalances`: Off-chain record of AI agent rebalancing events between protocols (Blend / DEX).
5. `yield_snapshots`: Periodic snapshots of total user assets for calculating APY trends and chart rendering.
6. `earnings_history`: Daily aggregated earnings per user.
7. `audit_logs`: Automatic audit log table populated via PostgreSQL trigger on writes.
8. `agent_state`: Agent key/value state (event listener cursor, state backups; see agent/src/ledgerCursor.ts).
9. `whatsapp_wallets`: WhatsApp custodial wallet storage.

## Security & Compliance
- **PII Encryption**: Phone numbers are hashed using SHA-256 with a salt before storing in `phone_hash`.
- **Row-Level Security (RLS)**: Enforces access control rules for Supabase clients so users can only view their own records.
- **Audit Logging**: `log_audit_trail()` trigger captures inserts, updates, and deletes.
- **Realtime Subscriptions**: Enabled via `supabase_realtime` publication for live UI portfolio updates.
