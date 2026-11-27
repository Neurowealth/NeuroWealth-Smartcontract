-- Restrict the rebalances SELECT policy to authenticated users only.
--
-- The original policy in 20260728000000_init_neurowealth_schema.sql omitted
-- `TO authenticated`, causing PostgreSQL to apply it to PUBLIC (including the
-- Supabase `anon` role) and exposing rebalance records to anonymous API clients.
--
-- Added as a new migration rather than editing the initial schema migration,
-- which has already been applied in deployed environments and would not re-run.

DROP POLICY IF EXISTS "Rebalances are viewable by all authenticated users" ON rebalances;

CREATE POLICY "Rebalances are viewable by all authenticated users"
  ON rebalances
  FOR SELECT
  TO authenticated
  USING (true);
