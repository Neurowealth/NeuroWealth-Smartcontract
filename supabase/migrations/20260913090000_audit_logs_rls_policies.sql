-- Issue #696: the audit_logs table has RLS enabled with no policies, so every
-- read and write is denied by default - including the backend's own inserts,
-- which means the audit trail the table exists for is empty and unreadable.
--
-- This is a new migration rather than an edit to
-- 20260728000000_init_neurowealth_schema.sql: that file has already been applied
-- in deployed environments, and Supabase records applied migrations, so changing
-- it would not re-run there.
--
-- Supabase's service_role bypasses RLS entirely, so the INSERT policy below is
-- not what grants the backend access; it documents the intended writer and keeps
-- the rule visible next to the table. Admin review is expressed through the
-- app_metadata role claim rather than user_metadata, because user_metadata is
-- writable by the user and would let anyone promote themselves to reviewer.

CREATE POLICY "Service role can append audit logs"
  ON audit_logs
  FOR INSERT
  TO service_role
  WITH CHECK (true);

CREATE POLICY "Admins can review audit logs"
  ON audit_logs
  FOR SELECT
  TO authenticated
  USING ((auth.jwt() -> 'app_metadata' ->> 'role') = 'admin');

-- Deliberately no UPDATE and no DELETE policy: an audit trail that can be
-- rewritten is not an audit trail, and RLS denies anything no policy allows.
