-- 0017_user_issue_state_touch_trigger.sql  (slice 2, §6)
--
-- BEFORE UPDATE trigger on `dp_user_issue_state` that bumps
-- `updated_at` to `now()` on every UPDATE. Today every writer in
-- `dp-store-pg` sets `updated_at = now()` explicitly; the trigger is
-- a defensive backstop so a future writer (or a manual operator
-- query during an incident) cannot leave a stale timestamp behind.
--
-- We rebuild the function as `OR REPLACE` so migrations stay
-- idempotent across re-runs in dev; the trigger itself is dropped
-- first for the same reason.

CREATE OR REPLACE FUNCTION dp_user_issue_state_touch_updated_at()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    NEW.updated_at := now();
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS dp_user_issue_state_touch_updated_at
    ON dp_user_issue_state;

CREATE TRIGGER dp_user_issue_state_touch_updated_at
    BEFORE UPDATE ON dp_user_issue_state
    FOR EACH ROW
    EXECUTE FUNCTION dp_user_issue_state_touch_updated_at();
