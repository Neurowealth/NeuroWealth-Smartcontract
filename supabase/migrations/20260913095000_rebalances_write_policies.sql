-- Issue #695: rebalances has RLS enabled and a single SELECT policy
-- (USING (true)), so no write is possible at all - the rebalance history the
-- table exists for can never be recorded by anything subject to RLS.
--
-- Added as a new migration rather than an edit to
-- 20260728000000_init_neurowealth_schema.sql, which has already been applied in
-- deployed environments and would not re-run.
--
-- On the "agent role" the issue asks for: no agent database role exists in this
-- schema, and nothing in the codebase writes this table yet, so a policy granted
-- to a role nobody holds would be dead configuration. The writer today is the
-- backend's service credential, which is what the FOR ALL policy below covers;
-- Supabase's service_role bypasses RLS anyway, so the policy documents the
-- intended writer rather than granting the access. If you want the agent to have
-- its own credential, that is a CREATE ROLE plus a secret - it needs a decision
-- and an owner, so I have not invented one here.
--
-- The public SELECT policy is deliberately left as it is: the issue records it as
-- existing by design. Worth noting for a separate decision that it makes
-- rebalance amounts and timestamps readable to anyone with the anon key.

CREATE POLICY "Service role manages rebalances"
  ON rebalances
  FOR ALL
  TO service_role
  USING (true)
  WITH CHECK (true);
