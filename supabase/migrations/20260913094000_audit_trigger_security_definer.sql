-- The audit trigger cannot record anything written by a role that is subject to
-- RLS: log_audit_trail() runs with the caller's privileges, so its INSERT into
-- audit_logs is filtered by that table's policies and the whole statement is
-- rolled back. Proven with RLS enabled on users: both a service_role and an
-- authenticated INSERT failed with
--   ERROR: new row violates row-level security policy for table "audit_logs"
--   CONTEXT: PL/pgSQL function log_audit_trail() line 3
-- which makes every policy on users/deposits/withdrawals unusable for any role
-- that does not bypass RLS.
--
-- SECURITY DEFINER runs the function as its owner instead, so the trail records
-- every write regardless of the writer's own policies. search_path is pinned to
-- prevent a caller-controlled schema from shadowing audit_logs. Users still
-- cannot forge audit rows: they have no INSERT policy on audit_logs themselves.
CREATE OR REPLACE FUNCTION log_audit_trail()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
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
$$;
