//! `verify` (0006 §9), levels 1 and 2, over a `LogStore` and its checkpoints.
//!
//! **Level 1, structure:** gapless offsets whose headers name their partition and
//! offset; the tree head over every leaf; the root at every checkpoint, each
//! checkpoint's origin and signature against the key chain, and consistency between
//! successive checkpoints; segment manifests against their segments' own trees; and,
//! for a `config` partition, the key chain itself.
//!
//! **Level 2, bodies:** every present part against the commitment; every erased part
//! against an `erase` record in the same partition (0015 §1); every client signature
//! against the actor keys the caller supplies; every instance attestation against the
//! key current when the record was appended (0040 §8).
//!
//! Level 3, projections, belongs to the crate that owns them.
//!
//! Verification collects problems rather than stopping at the first, so a report says
//! everything that is wrong with a partition.

use std::collections::BTreeSet;

use scatter_log::cbor::Value;
use scatter_log::hash::{Hash, hex};
use scatter_log::record::Record;
use scatter_log::store::{LogStore, Slot};
use scatter_log::tree::{Frontier, root_of};

use crate::attest::{AttestError, Attestation};
use crate::checkpoint::{SignedCheckpoint, segment_origin};
use crate::checkpoints::{CheckpointStore, CheckpointStoreError};
use crate::keys::{ChainError, KeyChain};
use crate::proof::{consistency_proof, verify_consistency};

/// The payload type of erasure records.
pub const PAYLOAD_ERASE: &str = "scatter:v0/erase";

/// How deep to go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Headers, the tree, checkpoints and the key chain.
    Structure,
    /// Also bodies, erasures and signatures.
    Bodies,
}

/// A lookup of an actor's public key by key ID.
pub type KeyLookup<'a> = &'a dyn Fn(&str) -> Option<[u8; 32]>;
/// A lookup of a payload type's part names beyond the three core parts.
pub type PartNames<'a> = &'a dyn Fn(&str) -> Vec<String>;

/// What `verify` needs besides the stores.
pub struct Verify<'a> {
    /// The partition.
    pub partition: u64,
    /// The checkpoint origin line of the partition.
    pub origin: &'a str,
    /// The instance key chain the checkpoints are signed under.
    pub chain: &'a KeyChain,
    /// How deep to go.
    pub level: Level,
    /// The public key of an actor's signing key by key ID, for client signatures; with
    /// `None`, client signatures are not checked.
    pub actor_keys: Option<KeyLookup<'a>>,
    /// The part names of a payload type beyond the three core parts, by index from 3,
    /// so that an `erase` record naming a type's extra part can be matched. With `None`,
    /// only `content`, `comment` and `attestation` are known.
    pub part_names: Option<PartNames<'a>>,
    /// Whether this is a `config` partition, whose key chain is rebuilt and checked.
    pub is_config: bool,
}

/// One thing wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Problem {
    /// The store could not read an offset below its head.
    Unreadable {
        /// The offset.
        offset: u64,
        /// What the store said.
        error: String,
    },
    /// A header names another partition or offset.
    Header {
        /// The offset read.
        offset: u64,
        /// What the header said.
        found: (u64, u64),
    },
    /// The store's head root is not the fold of its leaves.
    HeadRoot {
        /// The store's root.
        stored: String,
        /// The fold.
        computed: String,
    },
    /// A checkpoint's size is past the head.
    CheckpointSize {
        /// The size.
        size: u64,
    },
    /// A checkpoint's origin or signature is wrong.
    Checkpoint {
        /// The size.
        size: u64,
        /// Why.
        error: String,
    },
    /// A checkpoint's root is not the tree head at its size.
    CheckpointRoot {
        /// The size.
        size: u64,
    },
    /// Two successive checkpoints are not consistent.
    Consistency {
        /// The smaller size.
        from: u64,
        /// The larger.
        to: u64,
    },
    /// A segment manifest's origin, size, root or signature is wrong.
    Manifest {
        /// The segment.
        segment: u64,
        /// Why.
        error: String,
    },
    /// The key chain does not verify.
    Chain(ChainError),
    /// A body does not match its header's commitment.
    Commitment {
        /// The offset.
        offset: u64,
    },
    /// An erased part has no `erase` record accounting for it.
    Erasure {
        /// The offset.
        offset: u64,
        /// The part index.
        part: usize,
    },
    /// An `erase` record's content is not readable.
    EraseRecord {
        /// Its offset.
        offset: u64,
    },
    /// A signature is wrong, or by an unknown key.
    Signature {
        /// The offset.
        offset: u64,
        /// Why.
        error: AttestError,
    },
    /// An instance attestation is signed by a key that was not current at the time.
    StaleInstanceKey {
        /// The offset.
        offset: u64,
        /// The key ID used.
        key: String,
    },
    /// The checkpoint store failed.
    Checkpoints(String),
}

/// What `verify` found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Report {
    /// The partition.
    pub partition: u64,
    /// The head size.
    pub size: u64,
    /// Records present (not compacted).
    pub records: u64,
    /// Compacted offsets.
    pub compacted: u64,
    /// Records with at least one erased part.
    pub erased: u64,
    /// Checkpoints checked.
    pub checkpoints: usize,
    /// Manifests checked.
    pub manifests: usize,
    /// Signatures that verified.
    pub signatures: u64,
    /// Everything wrong.
    pub problems: Vec<Problem>,
}

impl Report {
    /// Whether nothing was wrong.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.problems.is_empty()
    }
}

/// Which parts an `erase` record covers, by offset or by key.
struct Erasure {
    at: u64,
    offsets: Vec<u64>,
    key: Option<String>,
    parts: Option<Vec<String>>,
}

fn erasure_of(record: &Record) -> Option<Erasure> {
    let content = record.body().content().value().ok()??;
    let targets = content.get("targets")?;
    let offsets = targets
        .get("offsets")
        .and_then(|o| match o {
            Value::Array(a) => Some(a.iter().filter_map(Value::as_u64).collect()),
            _ => None,
        })
        .unwrap_or_default();
    let key = targets
        .get("key")
        .and_then(Value::as_text)
        .map(str::to_owned);
    let parts = content.get("parts").and_then(|p| match p {
        Value::Array(a) => Some(
            a.iter()
                .filter_map(Value::as_text)
                .map(str::to_owned)
                .collect(),
        ),
        _ => None,
    });
    Some(Erasure {
        at: record.header().offset,
        offsets,
        key,
        parts,
    })
}

/// Verifies one partition.
pub async fn verify<S: LogStore, C: CheckpointStore>(
    store: &S,
    checkpoints: &C,
    what: &Verify<'_>,
) -> Report {
    let partition = what.partition;
    let mut report = Report {
        partition,
        ..Report::default()
    };
    let head = match store.head(partition).await {
        Ok(h) => h,
        Err(e) => {
            report.problems.push(Problem::Unreadable {
                offset: 0,
                error: e.to_string(),
            });
            return report;
        }
    };
    report.size = head.size;

    // Pass over every offset: headers, leaves, and what level 2 needs.
    let mut leaves: Vec<Hash> = Vec::with_capacity(usize::try_from(head.size).unwrap_or(0));
    let mut frontier = Frontier::new();
    let mut key_records: Vec<Record> = Vec::new();
    let mut erasures: Vec<Erasure> = Vec::new();
    let mut erased_parts: Vec<(u64, String, Option<String>, Vec<usize>)> = Vec::new();
    for offset in 0..head.size {
        let slot = match store.read(partition, offset).await {
            Ok(s) => s,
            Err(e) => {
                report.problems.push(Problem::Unreadable {
                    offset,
                    error: e.to_string(),
                });
                continue;
            }
        };
        let leaf = slot.leaf();
        leaves.push(leaf);
        frontier.push_leaf(leaf);
        let Slot::Record(record) = slot else {
            report.compacted += 1;
            continue;
        };
        report.records += 1;
        let h = record.header();
        if h.partition != partition || h.offset != offset {
            report.problems.push(Problem::Header {
                offset,
                found: (h.partition, h.offset),
            });
        }
        if what.is_config && KeyChain::is_key_record(&record) {
            key_records.push(record.clone());
        }
        if what.level < Level::Bodies {
            continue;
        }
        if record.body().verify(&h.commitment).is_err() {
            report.problems.push(Problem::Commitment { offset });
        }
        let erased: Vec<usize> = record
            .body()
            .parts()
            .iter()
            .enumerate()
            .filter(|(_, p)| p.is_erased())
            .map(|(i, _)| i)
            .collect();
        if !erased.is_empty() {
            report.erased += 1;
            erased_parts.push((offset, h.payload_type.clone(), h.key.clone(), erased));
        }
        if h.payload_type == PAYLOAD_ERASE {
            match erasure_of(&record) {
                Some(e) => erasures.push(e),
                None => report.problems.push(Problem::EraseRecord { offset }),
            }
        }
        check_signature(&record, what, &mut report);
    }

    if frontier.root() != head.root {
        report.problems.push(Problem::HeadRoot {
            stored: hex(&head.root),
            computed: hex(&frontier.root()),
        });
    }

    check_checkpoints(checkpoints, what, &leaves, &mut report).await;
    check_manifests(store, checkpoints, what, &leaves, &mut report).await;

    if what.is_config {
        let mut chain = KeyChain::new();
        for r in &key_records {
            if let Err(e) = chain.push(r) {
                report.problems.push(Problem::Chain(e));
                break;
            }
        }
    }

    if what.level >= Level::Bodies {
        check_erasures(what, &erased_parts, &erasures, &mut report);
    }
    report
}

async fn check_checkpoints<C: CheckpointStore>(
    checkpoints: &C,
    what: &Verify<'_>,
    leaves: &[Hash],
    report: &mut Report,
) {
    let list = match checkpoints.list(what.partition).await {
        Ok(l) => l,
        Err(e) => {
            report.problems.push(Problem::Checkpoints(e.to_string()));
            return;
        }
    };
    let verifiers = what.chain.verifiers(what.origin);
    let mut previous: Option<&SignedCheckpoint> = None;
    for cp in &list {
        report.checkpoints += 1;
        let size = cp.checkpoint.size;
        if let Err(e) = cp.verify(what.origin, &verifiers) {
            report.problems.push(Problem::Checkpoint {
                size,
                error: e.to_string(),
            });
        }
        let Ok(n) = usize::try_from(size) else {
            report.problems.push(Problem::CheckpointSize { size });
            continue;
        };
        if n > leaves.len() {
            report.problems.push(Problem::CheckpointSize { size });
            continue;
        }
        if root_of(&leaves[..n]) != cp.checkpoint.root {
            report.problems.push(Problem::CheckpointRoot { size });
        }
        if let Some(prev) = previous {
            let from = prev.checkpoint.size;
            if from > 0 && from < size {
                let proof = consistency_proof(leaves, from, size);
                if verify_consistency(
                    from,
                    size,
                    &prev.checkpoint.root,
                    &cp.checkpoint.root,
                    &proof,
                )
                .is_err()
                {
                    report
                        .problems
                        .push(Problem::Consistency { from, to: size });
                }
            }
        }
        previous = Some(cp);
    }
}

async fn check_manifests<S: LogStore, C: CheckpointStore>(
    store: &S,
    checkpoints: &C,
    what: &Verify<'_>,
    leaves: &[Hash],
    report: &mut Report,
) {
    let Ok(head) = store.head(what.partition).await else {
        return;
    };
    let manifests = match checkpoints.manifests(what.partition).await {
        Ok(m) => m,
        Err(e) => {
            report.problems.push(Problem::Checkpoints(e.to_string()));
            return;
        }
    };
    for (n, m) in &manifests {
        report.manifests += 1;
        let origin = segment_origin(what.origin, *n);
        let problem = |error: String| Problem::Manifest { segment: *n, error };
        if let Err(e) = m.verify(&origin, &what.chain.verifiers(&origin)) {
            report.problems.push(problem(e.to_string()));
            continue;
        }
        if !head.segments.is_full(*n, head.size) {
            report
                .problems
                .push(problem("the segment is not sealed".into()));
            continue;
        }
        let range = head.segments.range(*n);
        let (Ok(start), Ok(end)) = (usize::try_from(range.start), usize::try_from(range.end))
        else {
            report.problems.push(problem("segment out of range".into()));
            continue;
        };
        if end > leaves.len() {
            report
                .problems
                .push(problem("segment past the leaves read".into()));
            continue;
        }
        if m.checkpoint.size != head.segments.len()
            || root_of(&leaves[start..end]) != m.checkpoint.root
        {
            report
                .problems
                .push(problem("size or root does not match the segment".into()));
        }
    }
}

fn check_signature(record: &Record, what: &Verify<'_>, report: &mut Report) {
    let offset = record.header().offset;
    let attestation = match Attestation::of(record) {
        Ok(a) => a,
        Err(error) => {
            report.problems.push(Problem::Signature { offset, error });
            return;
        }
    };
    match &attestation {
        Attestation::Unsigned | Attestation::Unverifiable => {}
        Attestation::Client(_) => {
            if let Some(lookup) = what.actor_keys {
                match attestation.verify(record, lookup) {
                    Ok(true) => report.signatures += 1,
                    Ok(false) => {}
                    Err(error) => report.problems.push(Problem::Signature { offset, error }),
                }
            }
        }
        Attestation::Instance { signature, .. } => {
            let current = what.chain.current_at(record.header().appended_at);
            if current.is_none_or(|k| k.entry.key_id != signature.key) {
                report.problems.push(Problem::StaleInstanceKey {
                    offset,
                    key: signature.key.clone(),
                });
            }
            match attestation.verify(record, |id| what.chain.lookup(id)) {
                Ok(true) => report.signatures += 1,
                Ok(false) => {}
                Err(error) => report.problems.push(Problem::Signature { offset, error }),
            }
        }
    }
}

fn part_index(name: &str, payload_type: &str, what: &Verify<'_>) -> Option<usize> {
    match name {
        "content" => Some(0),
        "comment" => Some(1),
        "attestation" => Some(2),
        other => what.part_names.and_then(|f| {
            f(payload_type)
                .iter()
                .position(|n| n == other)
                .map(|i| i + 3)
        }),
    }
}

fn check_erasures(
    what: &Verify<'_>,
    erased_parts: &[(u64, String, Option<String>, Vec<usize>)],
    erasures: &[Erasure],
    report: &mut Report,
) {
    for (offset, payload_type, key, parts) in erased_parts {
        // What the erase records covering this record account for; `None` is all parts.
        let mut covered: Option<BTreeSet<usize>> = Some(BTreeSet::new());
        for e in erasures {
            let by_offset = e.offsets.contains(offset);
            let by_key = key
                .as_deref()
                .is_some_and(|k| e.key.as_deref() == Some(k) && e.at > *offset);
            if !(by_offset || by_key) {
                continue;
            }
            match (&e.parts, &mut covered) {
                (None, _) => covered = None,
                (Some(names), Some(set)) => {
                    set.extend(
                        names
                            .iter()
                            .filter_map(|n| part_index(n, payload_type, what)),
                    );
                }
                (Some(_), None) => {}
            }
        }
        let Some(set) = covered else {
            continue;
        };
        for &part in parts {
            if !set.contains(&part) {
                report.problems.push(Problem::Erasure {
                    offset: *offset,
                    part,
                });
            }
        }
    }
}

impl From<CheckpointStoreError> for Problem {
    fn from(e: CheckpointStoreError) -> Self {
        Self::Checkpoints(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use scatter_log::body::{ATTESTATION, Body, COMMENT};
    use scatter_log::cbor;
    use scatter_log::conformance::draft;
    use scatter_log::genesis::Genesis;
    use scatter_log::header::CONFIG_PARTITION;
    use scatter_log::memory::MemoryStore;
    use scatter_log::registry::{GraphRegistry, KeyEntry, Scope, key_id};
    use scatter_log::store::Draft;
    use scatter_log::tree::Segments;

    use crate::attest::{client_signature, instance_signature};
    use crate::checkpoint::Checkpoint;
    use crate::checkpoints::{MemoryCheckpoints, checkpoint, segment_manifest};

    fn erase_draft(n: u64, content: &Value) -> Draft {
        Draft {
            appended_at: 1_790_000_000_000_000 + n,
            payload_type: PAYLOAD_ERASE.into(),
            key: None,
            revid: None,
            logid: None,
            page_id: None,
            body: Body::core(
                content,
                &Value::text("privacy"),
                &Value::map(vec![(Value::text("actor"), Value::text("local:1"))]),
            )
            .unwrap(),
        }
    }

    struct Fixture {
        store: MemoryStore,
        cps: MemoryCheckpoints,
        chain: KeyChain,
        key: SigningKey,
    }

    async fn fixture() -> Fixture {
        let key = SigningKey::from_bytes(&[11; 32]);
        let entry = KeyEntry::ed25519(key.verifying_key().to_bytes());
        let mut store = MemoryStore::new();
        let registry = GraphRegistry::embedded();
        Genesis {
            partition: CONFIG_PARTITION,
            scope: Scope::Instance,
            key: entry,
            segment_exponent: 16,
            appended_at: 1,
            attestation: Value::map(vec![(
                Value::text("actor"),
                Value::text("instance:scatter"),
            )]),
            comment: None,
        }
        .append(&mut store, &registry)
        .await
        .unwrap();
        let chain =
            KeyChain::from_records([store.read(0, 0).await.unwrap().record().unwrap()]).unwrap();
        store
            .create_partition(7, Segments::new(2).unwrap())
            .await
            .unwrap();
        let mut cps = MemoryCheckpoints::new();
        for n in 0..9 {
            store.append(7, draft(n)).await.unwrap();
            if n % 4 == 3 {
                cps.put(
                    7,
                    &checkpoint(&store, 7, "h/log/local", &key).await.unwrap(),
                )
                .await
                .unwrap();
            }
        }
        cps.put(
            7,
            &checkpoint(&store, 7, "h/log/local", &key).await.unwrap(),
        )
        .await
        .unwrap();
        cps.put_manifest(
            7,
            0,
            &segment_manifest(&store, 7, 0, "h/log/local", &key)
                .await
                .unwrap(),
        )
        .await
        .unwrap();
        cps.put_manifest(
            7,
            1,
            &segment_manifest(&store, 7, 1, "h/log/local", &key)
                .await
                .unwrap(),
        )
        .await
        .unwrap();
        Fixture {
            store,
            cps,
            chain,
            key,
        }
    }

    fn what(chain: &KeyChain, level: Level) -> Verify<'_> {
        Verify {
            partition: 7,
            origin: "h/log/local",
            chain,
            level,
            actor_keys: None,
            part_names: None,
            is_config: false,
        }
    }

    #[test]
    fn a_clean_partition_verifies() {
        pollster::block_on(async {
            let f = fixture().await;
            let report = verify(&f.store, &f.cps, &what(&f.chain, Level::Bodies)).await;
            assert!(report.is_clean(), "{:?}", report.problems);
            assert_eq!(report.size, 9);
            assert_eq!(report.records, 9);
            assert_eq!(report.checkpoints, 3);
            assert_eq!(report.manifests, 2);
            // The config partition verifies with its chain.
            let config = Verify {
                partition: 0,
                origin: "farm/instance/log/config",
                is_config: true,
                ..what(&f.chain, Level::Bodies)
            };
            let report = verify(&f.store, &f.cps, &config).await;
            assert!(report.is_clean(), "{:?}", report.problems);
            assert_eq!(report.records, 2);
        });
    }

    #[test]
    fn checkpoint_problems_are_found() {
        pollster::block_on(async {
            let mut f = fixture().await;
            // A checkpoint signed by another key, one with a wrong root, one past the head.
            let other = SigningKey::from_bytes(&[12; 32]);
            let forged = checkpoint(&f.store, 7, "h/log/local", &other)
                .await
                .unwrap();
            f.cps.put(7, &forged).await.unwrap();
            let wrong_root = Checkpoint {
                origin: "h/log/local".into(),
                size: 5,
                root: [1; 32],
            }
            .sign(&f.key)
            .unwrap();
            f.cps.put(7, &wrong_root).await.unwrap();
            let past = Checkpoint {
                origin: "h/log/local".into(),
                size: 50,
                root: [1; 32],
            }
            .sign(&f.key)
            .unwrap();
            f.cps.put(7, &past).await.unwrap();
            let report = verify(&f.store, &f.cps, &what(&f.chain, Level::Structure)).await;
            assert!(
                report
                    .problems
                    .contains(&Problem::CheckpointRoot { size: 5 })
            );
            assert!(
                report
                    .problems
                    .iter()
                    .any(|p| matches!(p, Problem::Checkpoint { size: 9, .. }))
            );
            assert!(
                report
                    .problems
                    .contains(&Problem::CheckpointSize { size: 50 })
            );
            // The wrong root at 5 also breaks consistency 4 → 5 and 5 → 8.
            assert!(
                report
                    .problems
                    .contains(&Problem::Consistency { from: 4, to: 5 })
            );
            assert!(
                report
                    .problems
                    .contains(&Problem::Consistency { from: 5, to: 8 })
            );
            // A manifest under the wrong origin.
            let bad = Checkpoint {
                origin: segment_origin("h/log/local", 1),
                size: 4,
                root: [2; 32],
            }
            .sign(&f.key)
            .unwrap();
            f.cps.put_manifest(7, 1, &bad).await.unwrap();
            let report = verify(&f.store, &f.cps, &what(&f.chain, Level::Structure)).await;
            assert!(
                report
                    .problems
                    .iter()
                    .any(|p| matches!(p, Problem::Manifest { segment: 1, .. }))
            );
        });
    }

    #[test]
    fn erasures_need_erase_records() {
        pollster::block_on(async {
            let mut f = fixture().await;
            f.store.erase_parts(7, 2, &[ATTESTATION]).await.unwrap();
            f.store
                .erase_parts(7, 3, &[COMMENT, ATTESTATION])
                .await
                .unwrap();
            f.store.erase_parts(7, 5, &[COMMENT]).await.unwrap();
            let report = verify(&f.store, &f.cps, &what(&f.chain, Level::Bodies)).await;
            assert_eq!(
                report.problems,
                vec![
                    Problem::Erasure { offset: 2, part: 2 },
                    Problem::Erasure { offset: 3, part: 1 },
                    Problem::Erasure { offset: 3, part: 2 },
                    Problem::Erasure { offset: 5, part: 1 },
                ]
            );
            // Offset 2 by offsets and part name; offset 3 by key, all parts; offset 5 by
            // offsets, all parts.
            f.store
                .append(
                    7,
                    erase_draft(
                        20,
                        &Value::map(vec![
                            (
                                Value::text("targets"),
                                Value::map(vec![(
                                    Value::text("offsets"),
                                    Value::Array(vec![Value::Int(2)]),
                                )]),
                            ),
                            (
                                Value::text("parts"),
                                Value::Array(vec![Value::text("attestation")]),
                            ),
                        ]),
                    ),
                )
                .await
                .unwrap();
            f.store
                .append(
                    7,
                    erase_draft(
                        21,
                        &Value::map(vec![(
                            Value::text("targets"),
                            Value::map(vec![(Value::text("key"), Value::text("Q3"))]),
                        )]),
                    ),
                )
                .await
                .unwrap();
            let report = verify(&f.store, &f.cps, &what(&f.chain, Level::Bodies)).await;
            assert_eq!(
                report.problems,
                vec![Problem::Erasure { offset: 5, part: 1 }]
            );
            f.store
                .append(
                    7,
                    erase_draft(
                        22,
                        &Value::map(vec![(
                            Value::text("targets"),
                            Value::map(vec![(
                                Value::text("offsets"),
                                Value::Array(vec![Value::Int(5)]),
                            )]),
                        )]),
                    ),
                )
                .await
                .unwrap();
            let report = verify(&f.store, &f.cps, &what(&f.chain, Level::Bodies)).await;
            assert!(report.is_clean(), "{:?}", report.problems);
            assert_eq!(report.erased, 3);
            // Level 1 does not look at bodies at all.
            f.store.erase_parts(7, 6, &[COMMENT]).await.unwrap();
            assert!(
                verify(&f.store, &f.cps, &what(&f.chain, Level::Structure))
                    .await
                    .is_clean()
            );
        });
    }

    #[test]
    fn signatures_are_checked() {
        pollster::block_on(async {
            let mut f = fixture().await;
            let actor = SigningKey::from_bytes(&[13; 32]);
            let actor_pk = actor.verifying_key().to_bytes();
            let actor_id = key_id(&actor_pk);
            let content = Value::map(vec![(Value::text("op"), Value::text("put"))]);
            let comment = Value::text("signed");
            let content_bytes = cbor::encode(&content).unwrap();
            let comment_bytes = cbor::encode(&comment).unwrap();
            // A good client signature, a bad one, and an instance attestation.
            let good = client_signature(&actor, &actor_id, &content_bytes, &comment_bytes);
            let bad = client_signature(
                &SigningKey::from_bytes(&[14; 32]),
                &actor_id,
                &content_bytes,
                &comment_bytes,
            );
            let authority = Value::map(vec![
                (Value::text("partition"), Value::Int(0)),
                (Value::text("offset"), Value::Int(1)),
            ]);
            let instance = instance_signature(
                &f.key,
                &f.chain.current().unwrap().entry.key_id,
                &content_bytes,
                &comment_bytes,
                &cbor::encode(&authority).unwrap(),
            );
            for (n, attestation) in [
                Value::map(vec![
                    (Value::text("actor"), Value::text("local:1")),
                    (Value::text("signature"), good),
                ]),
                Value::map(vec![
                    (Value::text("actor"), Value::text("local:1")),
                    (Value::text("signature"), bad),
                ]),
                Value::map(vec![
                    (Value::text("actor"), Value::text("instance:scatter")),
                    (Value::text("authority"), authority),
                    (Value::text("signature"), instance),
                ]),
            ]
            .into_iter()
            .enumerate()
            {
                let mut d = draft(30 + n as u64);
                d.body = Body::core(&content, &comment, &attestation).unwrap();
                f.store.append(7, d).await.unwrap();
            }
            let lookup = |id: &str| (id == actor_id).then_some(actor_pk);
            let w = Verify {
                actor_keys: Some(&lookup),
                ..what(&f.chain, Level::Bodies)
            };
            let report = verify(&f.store, &f.cps, &w).await;
            assert_eq!(report.signatures, 2);
            assert_eq!(report.problems.len(), 1);
            assert!(matches!(
                report.problems[0],
                Problem::Signature {
                    offset: 10,
                    error: AttestError::BadSignature(_)
                }
            ));
            // Without actor keys, client signatures are not checked; the instance one is.
            let report = verify(&f.store, &f.cps, &what(&f.chain, Level::Bodies)).await;
            assert_eq!(report.signatures, 1);
            assert!(report.is_clean());
        });
    }
}
