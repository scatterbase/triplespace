-- Origins of partitions created before `log.partition.origin` existed and never signed
-- (01 §4.2; 0006 A29): `scatter-log-postgres`'s migration of the same name took the rest
-- from their first checkpoint or manifest. These are derived from the tenant's current
-- host, or the farm's for an instance partition, as their first checkpoint would have
-- derived them: the best that can be done, since the host at creation was not recorded.
-- A tenant the registry does not list yet keeps NULL and is derived when first signed.
-- Idempotent.

UPDATE log.partition p
   SET origin = regexp_replace(regexp_replace(r.config->>'base', '^[A-Za-z][A-Za-z0-9+.-]*://', ''),
                               '[/?#].*$', '')
                || '/log/' || p.name
  FROM view.registry r
 WHERE r.tenant = '' AND r.kind = 'tenant' AND r.code = p.tenant
   AND p.tenant IS NOT NULL AND p.origin IS NULL;

UPDATE log.partition p
   SET origin = regexp_replace(regexp_replace(r.config->>'base', '^[A-Za-z][A-Za-z0-9+.-]*://', ''),
                               '[/?#].*$', '')
                || '/instance/log/' || p.name
  FROM view.registry r
 WHERE r.tenant = '' AND r.kind = 'farm' AND r.code = 'farm'
   AND p.tenant IS NULL AND p.origin IS NULL;
