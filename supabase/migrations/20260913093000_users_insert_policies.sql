-- Issue #694: the users table has SELECT and UPDATE policies keyed on
-- auth.uid() = id but no INSERT policy, so a signed-in user cannot create their
-- own row and the only writer is a role that bypasses RLS.
--
-- Added as a new migration rather than an edit to
-- 20260728000000_init_neurowealth_schema.sql: that file has already been applied
-- in deployed environments and Supabase records applied migrations, so editing it
-- would not re-run there.
--
-- Least privilege, in both directions:
--   * an authenticated user may insert exactly one row, their own, which is what
--     auth.uid() = id on the WITH CHECK enforces;
--   * the service role (the backend's own sign-up path, and Supabase's
--     service key) may insert any row.
-- No DELETE policy is added: deleting an account is an administrative action that
-- belongs to the service role, and RLS denies users by default.

CREATE POLICY "Users can insert their own profile"
  ON users
  FOR INSERT
  TO authenticated
  WITH CHECK (auth.uid() = id);

CREATE POLICY "Service role can insert user profiles"
  ON users
  FOR INSERT
  TO service_role
  WITH CHECK (true);
