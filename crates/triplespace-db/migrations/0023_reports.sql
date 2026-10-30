-- Reports (0047 §4.3; 03 §4.10): the entries of projection-backed reports, and each
-- report's state per tenant.
--
-- `view.report_entry` holds one row per entry, keyed by what the report lists: a title as
-- `view.page_link` writes it (`{ns}:{title}`), or a category name. `sort_value` orders a
-- count report (largest first); an alphabetical report leaves it null and is read in key
-- order, which the one index serves either way.
--
-- `view.report_state` has a row per report a tenant's reports have been built for. Its
-- presence is what turns the tenant's reports on: until the first build, the page and ACL
-- projections queue no report work, so an adoption or a bulk load costs nothing here, and
-- the first read builds from `view` (0047 §4.3: off during bootstrap and bulk work until
-- turned on). Both tables are view-derived: a rebuild is a scan of the tables they read,
-- never a replay of the log.

CREATE TABLE IF NOT EXISTS view.report_entry (
  tenant     text   NOT NULL DEFAULT '',
  report     text   NOT NULL,                   -- the registry name, `WantedPages`
  scope      text   NOT NULL DEFAULT 'local',   -- the graph set (0047 §4.2)
  key        text   NOT NULL,
  sort_value bigint,
  detail     jsonb,
  PRIMARY KEY (tenant, report, scope, key)
);
CREATE INDEX IF NOT EXISTS report_entry_order
  ON view.report_entry (tenant, report, scope, sort_value DESC, key);

CREATE TABLE IF NOT EXISTS view.report_state (
  tenant      text   NOT NULL DEFAULT '',
  report      text   NOT NULL,
  scope       text   NOT NULL DEFAULT 'local',
  mode        text   NOT NULL,                  -- projection | batch
  computed_at timestamptz,                      -- when the last full build finished
  row_count   bigint,
  PRIMARY KEY (tenant, report, scope)
);

-- The report worker takes a tenant's work in order; the queue is shared by projections.
CREATE INDEX IF NOT EXISTS projection_work_tenant
  ON ops.projection_work (projection, tenant, id);
