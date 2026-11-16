-- What the services need once each connects as its own role (ADR 0013 §4, A99), which
-- connecting as the owner hid.
--
--   ts_accounts  the sequences in `private` (`private.login_id`, which numbers instance
--                logins): 0004_roles granted the tables only.
--   ts_notify    `private.preference`, read: the notification matrix and the auto-watch
--                flags are preferences (0027 §1), and addressing reads them.

GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA private TO ts_accounts;
ALTER DEFAULT PRIVILEGES IN SCHEMA private GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO ts_accounts;
GRANT SELECT ON private.preference TO ts_notify;
