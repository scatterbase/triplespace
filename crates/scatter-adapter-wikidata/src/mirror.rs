//! Mirror mode (0002 §8.4): a provider's entities under its prefixed IDs, with the
//! upstream version from the dump's `lastrevid` and `modified`.

use scatter_wikibase_changeset::{Adapter, Upstream, WikibaseAdapter};
use scatter_wikibase_model::entity::{Entity, ParsedEntity};

use crate::DumpError;

/// A dump entity as the mirror stores it: IDs rewritten to the provider form, and its
/// upstream revision. `None` for the revision when the dump carries no `lastrevid`.
pub fn mirror_entity(
    parsed: ParsedEntity,
    adapter: &WikibaseAdapter,
) -> Result<(Entity, Upstream), DumpError> {
    let mut entity = parsed.entity;
    adapter.rewrite_entity(&mut entity)?;
    let upstream = match parsed.page.as_ref().and_then(|p| p.lastrevid) {
        Some(revid) => Upstream::revision(
            revid,
            parsed.page.as_ref().and_then(|p| p.modified.as_deref()),
        ),
        None => Upstream::default(),
    };
    Ok((entity, upstream))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_and_versions() {
        let parsed = Entity::from_json(
            r#"{"type":"item","id":"Q42","labels":{},"claims":{"P31":[{"mainsnak":{"snaktype":"value","property":"P31",
                "datavalue":{"value":{"entity-type":"item","id":"Q5"},"type":"wikibase-entityid"},"datatype":"wikibase-item"},
                "type":"statement","rank":"normal","id":"Q42$F078E5B3-F9A8-480E-B7AC-D97778CBBEF9"}]},
                "lastrevid":2148573921,"modified":"2026-09-30T12:00:00Z"}"#,
        )
        .unwrap();
        let wd = WikibaseAdapter::new("wikidata").unwrap();
        let (entity, upstream) = mirror_entity(parsed, &wd).unwrap();
        assert_eq!(entity.id.as_str(), "WDQ42");
        assert_eq!(upstream.revid, Some(2_148_573_921));
        assert_eq!(upstream.time.as_deref(), Some("2026-09-30T12:00:00Z"));
        let s = entity.all_statements().next().unwrap();
        assert_eq!(
            s.id.as_ref().unwrap().as_str(),
            "WDQ42$F078E5B3-F9A8-480E-B7AC-D97778CBBEF9"
        );
    }
}
