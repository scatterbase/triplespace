//! The hasher for data mirrored from a provider (0006 §2; 0115 §13): its IDs hashed in the
//! bare form the provider wrote them, so that a recomputed hash equals the provider's own.

use scatter_providers::Provider;
use scatter_wikibase_model::Hasher;

// Without the `providers` feature, a caller builds the hasher with
// `Hasher::mirrored_from` or `Hasher::mirrored_from_source` itself.

/// The hasher for data mirrored from `provider`: a registry provider's IDs without its code
/// (`WDQ42` as `Q42`), an entity source's without its name (`lb:Q1` as `Q1`, 0078 §2).
#[must_use]
pub fn hasher_for(provider: &Provider) -> Hasher {
    if provider.source {
        Hasher::mirrored_from_source(&provider.slug)
    } else {
        Hasher::mirrored_from(&provider.code)
    }
}
