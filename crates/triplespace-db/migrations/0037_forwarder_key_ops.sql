-- Forwarder keys move from `private` to `ops` (ADR 0013 §4, A101). A forwarder key lets a
-- proxy vouch for the client address it forwards (0057 §10): it belongs to no account, and
-- the public API checks it on every forwarded request. Kept in `private`, that check needed
-- the accounts service's connection; in `ops` the public API's role reads the hash as it
-- reads the rest of the operational state, and the key itself never reaches the database.
-- The table keeps its rows, indexes and constraint names.

ALTER TABLE private.forwarder_key SET SCHEMA ops;
COMMENT ON TABLE ops.forwarder_key IS 'forwarder keys: hashes of the secrets proxies present (0057 §10)';
