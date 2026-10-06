-- Lookups the projections make that the primary keys do not serve (0013 §5).
-- Idempotent: every statement tolerates having run before.

-- The resolution projection replaces a subject's identifier rows by (tenant, entity_id);
-- the primary key leads with (tenant, property, value_key), so without this the delete
-- scanned the tenant's whole table, once per record.
CREATE INDEX IF NOT EXISTS identifier_entity ON view.identifier (tenant, entity_id);

-- A record in a shared (instance) partition recomputes the row of every tenant that
-- overlays the subject (0018 §6): those tenants are found by ID alone, across tenants,
-- which the (tenant, …) primary keys cannot answer.
CREATE INDEX IF NOT EXISTS entity_by_id ON view.entity (id);
CREATE INDEX IF NOT EXISTS entity_source_by_entity ON view.entity_source (entity_id);
