-- Identifier keys under the resolver bound to their property (06 §3.1; ADR 0029 A12): the
-- entity projection keys Wikidata's DOI property, mirrored as `WDP356`, by the DOI
-- resolver's normalizer, which casefolds, so that viewing `DOI:10.1000/xyz` finds a value
-- Wikidata writes as `10.1000/XYZ`. Rows written before were keyed by the value as written.
-- This re-keys the bare ASCII DOIs among them, which is all but a few of Wikidata's; any
-- other row takes its new key when its entity is next composed, or by `triplespace rebuild`.
-- A key is `external-id`, the unit separator, then the key.
INSERT INTO view.identifier (tenant, property, value_key, entity_id, graph, statement_id)
SELECT tenant, property, 'external-id' || chr(31) || lower(substr(value_key, 13)),
       entity_id, graph, statement_id
FROM view.identifier
WHERE property = 'WDP356'
  AND value_key ~ ('^external-id' || chr(31) || '10\.[0-9]{4,9}/[!-~]+$')
  AND value_key <> lower(value_key)
ON CONFLICT DO NOTHING;

DELETE FROM view.identifier
WHERE property = 'WDP356'
  AND value_key ~ ('^external-id' || chr(31) || '10\.[0-9]{4,9}/[!-~]+$')
  AND value_key <> lower(value_key);
