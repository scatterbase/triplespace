//! The `records` export (0047 §8; 21 §4.2; 0006 §9): a selection of records, each with an
//! inclusion proof against its partition's latest checkpoint, with that checkpoint and
//! the key records that check it, in one canonical-CBOR document. A third party holding
//! only this file and the instance's published key chain can confirm that each record is
//! exactly what the log holds.
//!
//! ```text
//! {"format": "triplespace-records", "version": 1, "created": µs,
//!  "keys": [record, …],                       the instance's `key:` records
//!  "missing": [text, …],                      what was asked for and is not there
//!  "partitions": [{
//!     "partition": u64, "name": text, "origin": text,
//!     "checkpoint": text | null,              the C2SP signed note
//!     "nodes": [bytes, …],                    every proof node, once
//!     "records": [{"offset": u64,
//!                  "record": record | null,   null: the leaf alone
//!                  "leaf": bytes | null,      when "record" is null
//!                  "withheld": [u64, …],      parts carried as their commitments
//!                  "path": [u64, …]}, …],     the proof, as positions in "nodes"
//!     "pending": [u64, …]}, …]}               offsets past the checkpoint, left out
//! ```
//!
//! A part the viewer may not read is erased in the record, so the leaf and the proof hold
//! (01 §8); a record of a target a confidential restriction hides is its leaf alone, as a
//! compacted one is (0103 A1). [`verify`] checks each checkpoint against the key chain,
//! each record's header against its body and offset, and each leaf against its checkpoint.

use scatter_log::cbor::{self, Value};
use scatter_log::hash::Hash;
use scatter_log::record::Record;

use crate::checkpoint::SignedCheckpoint;
use crate::keys::KeyChain;
use crate::proof::verify_inclusion;

/// The `format` field.
pub const FORMAT: &str = "triplespace-records";
/// The format version this crate writes and reads.
pub const VERSION: u64 = 1;
/// The media type of the document.
pub const MEDIA_TYPE: &str = "application/vnd.triplespace.records+cbor";

/// One offset as an export shows it (01 §8; 0103 A1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shown {
    /// Only the leaf: a compacted record, or one of a target a confidential restriction
    /// hides, which reads the same.
    Leaf(Hash),
    /// The record, with the parts the reader may not read reduced to their commitments.
    Record {
        /// The record, those parts erased.
        record: Record,
        /// The parts withheld, apart from those erased in the log.
        withheld: Vec<usize>,
    },
}

impl Shown {
    /// The leaf the log holds at the offset.
    #[must_use]
    pub fn leaf(&self) -> Hash {
        match self {
            Self::Leaf(h) => *h,
            Self::Record { record, .. } => record.leaf(),
        }
    }
}

/// One record of a partition, with its proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The offset.
    pub offset: u64,
    /// What is shown of it.
    pub shown: Shown,
    /// The inclusion proof, leaf level first, as positions in the partition's `nodes`.
    pub path: Vec<u64>,
}

/// The records of one partition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordsPartition {
    /// The partition ID.
    pub partition: u64,
    /// Its name on its tenant.
    pub name: String,
    /// Its origin line.
    pub origin: String,
    /// The checkpoint every entry is proved against; none before the first is signed.
    pub checkpoint: Option<SignedCheckpoint>,
    /// Every node the proofs use, once.
    pub nodes: Vec<Hash>,
    /// The records, ascending by offset.
    pub entries: Vec<Entry>,
    /// Selected offsets past the checkpoint, left out until one covers them.
    pub pending: Vec<u64>,
}

/// A `records` export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordsExport {
    /// When it was made, µs since the epoch.
    pub created: u64,
    /// The instance's `key:` records, which sign every checkpoint.
    pub keys: Vec<Record>,
    /// What was asked for and is not there, as asked (titles, IDs).
    pub missing: Vec<String>,
    /// The partitions with selected records.
    pub partitions: Vec<RecordsPartition>,
}

/// Why bytes are not a `records` export.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RecordsError {
    /// Not canonical CBOR.
    #[error("records: {0}")]
    Cbor(String),
    /// A field is missing or of the wrong type.
    #[error("records: {0}")]
    Shape(String),
    /// Another format or a later version.
    #[error("records: format `{0}` version {1} is not one this verifier reads")]
    Version(String, u64),
}

fn u64s(items: &[u64]) -> Value {
    Value::Array(items.iter().map(|&n| Value::Int(i128::from(n))).collect())
}

fn field<'a>(v: &'a Value, key: &str) -> Result<&'a Value, RecordsError> {
    v.get(key)
        .ok_or_else(|| RecordsError::Shape(format!("no `{key}`")))
}

fn number(v: &Value, key: &str) -> Result<u64, RecordsError> {
    field(v, key)?
        .as_u64()
        .ok_or_else(|| RecordsError::Shape(format!("`{key}` is not a number")))
}

fn text(v: &Value, key: &str) -> Result<String, RecordsError> {
    field(v, key)?
        .as_text()
        .map(str::to_string)
        .ok_or_else(|| RecordsError::Shape(format!("`{key}` is not text")))
}

fn array<'a>(v: &'a Value, key: &str) -> Result<&'a [Value], RecordsError> {
    match field(v, key)? {
        Value::Array(a) => Ok(a),
        _ => Err(RecordsError::Shape(format!("`{key}` is not an array"))),
    }
}

fn numbers(v: &Value, key: &str) -> Result<Vec<u64>, RecordsError> {
    array(v, key)?
        .iter()
        .map(|n| {
            n.as_u64()
                .ok_or_else(|| RecordsError::Shape(format!("`{key}` holds a non-number")))
        })
        .collect()
}

fn hash_of(v: &Value, what: &str) -> Result<Hash, RecordsError> {
    v.as_bytes()
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| RecordsError::Shape(format!("{what} is not 32 bytes")))
}

fn record_of(v: &Value, what: &str) -> Result<Record, RecordsError> {
    Record::from_value(v).map_err(|e| RecordsError::Shape(format!("{what}: {e}")))
}

impl RecordsExport {
    /// The document as a CBOR value.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let partitions = self
            .partitions
            .iter()
            .map(|p| {
                let entries = p
                    .entries
                    .iter()
                    .map(|e| {
                        let (record, leaf, withheld) = match &e.shown {
                            Shown::Leaf(h) => (Value::Null, Value::Bytes(h.to_vec()), Vec::new()),
                            Shown::Record { record, withheld } => (
                                record.to_value(),
                                Value::Null,
                                withheld.iter().map(|&i| i as u64).collect(),
                            ),
                        };
                        Value::map(vec![
                            (Value::text("offset"), Value::Int(i128::from(e.offset))),
                            (Value::text("record"), record),
                            (Value::text("leaf"), leaf),
                            (Value::text("withheld"), u64s(&withheld)),
                            (Value::text("path"), u64s(&e.path)),
                        ])
                    })
                    .collect();
                Value::map(vec![
                    (
                        Value::text("partition"),
                        Value::Int(i128::from(p.partition)),
                    ),
                    (Value::text("name"), Value::text(&p.name)),
                    (Value::text("origin"), Value::text(&p.origin)),
                    (
                        Value::text("checkpoint"),
                        p.checkpoint
                            .as_ref()
                            .map_or(Value::Null, |c| Value::text(&c.to_text())),
                    ),
                    (
                        Value::text("nodes"),
                        Value::Array(p.nodes.iter().map(|h| Value::Bytes(h.to_vec())).collect()),
                    ),
                    (Value::text("records"), Value::Array(entries)),
                    (Value::text("pending"), u64s(&p.pending)),
                ])
            })
            .collect();
        Value::map(vec![
            (Value::text("format"), Value::text(FORMAT)),
            (Value::text("version"), Value::Int(i128::from(VERSION))),
            (Value::text("created"), Value::Int(i128::from(self.created))),
            (
                Value::text("keys"),
                Value::Array(self.keys.iter().map(Record::to_value).collect()),
            ),
            (
                Value::text("missing"),
                Value::Array(self.missing.iter().map(|m| Value::text(m)).collect()),
            ),
            (Value::text("partitions"), Value::Array(partitions)),
        ])
    }

    /// The canonical bytes.
    ///
    /// # Panics
    ///
    /// Never: every field encodes.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&self.to_value()).expect("a records export encodes")
    }

    /// Reads a document. A record whose body does not match its header's commitment is
    /// refused here, as a bundle refuses one when it is opened.
    ///
    /// # Errors
    ///
    /// Bytes that are not canonical CBOR, another format or version, or a field of the
    /// wrong shape.
    pub fn decode(bytes: &[u8]) -> Result<Self, RecordsError> {
        let v = cbor::decode(bytes).map_err(|e| RecordsError::Cbor(e.to_string()))?;
        let format = text(&v, "format")?;
        let version = number(&v, "version")?;
        if format != FORMAT || version != VERSION {
            return Err(RecordsError::Version(format, version));
        }
        let keys = array(&v, "keys")?
            .iter()
            .map(|k| record_of(k, "a key record"))
            .collect::<Result<_, _>>()?;
        let missing = array(&v, "missing")?
            .iter()
            .map(|m| {
                m.as_text()
                    .map(str::to_string)
                    .ok_or_else(|| RecordsError::Shape("`missing` holds a non-text".into()))
            })
            .collect::<Result<_, _>>()?;
        let mut partitions = Vec::new();
        for p in array(&v, "partitions")? {
            let checkpoint = match field(p, "checkpoint")? {
                Value::Null => None,
                Value::Text(t) => Some(
                    SignedCheckpoint::parse(t.as_bytes())
                        .map_err(|e| RecordsError::Shape(e.to_string()))?,
                ),
                _ => return Err(RecordsError::Shape("`checkpoint` is not text".into())),
            };
            let nodes = array(p, "nodes")?
                .iter()
                .map(|n| hash_of(n, "a node"))
                .collect::<Result<_, _>>()?;
            let mut entries = Vec::new();
            for e in array(p, "records")? {
                let offset = number(e, "offset")?;
                let shown = match field(e, "record")? {
                    Value::Null => Shown::Leaf(hash_of(field(e, "leaf")?, "a leaf")?),
                    r => Shown::Record {
                        record: record_of(r, &format!("the record at {offset}"))?,
                        withheld: numbers(e, "withheld")?
                            .into_iter()
                            .map(|i| usize::try_from(i).unwrap_or(usize::MAX))
                            .collect(),
                    },
                };
                entries.push(Entry {
                    offset,
                    shown,
                    path: numbers(e, "path")?,
                });
            }
            partitions.push(RecordsPartition {
                partition: number(p, "partition")?,
                name: text(p, "name")?,
                origin: text(p, "origin")?,
                checkpoint,
                nodes,
                entries,
                pending: numbers(p, "pending")?,
            });
        }
        Ok(Self {
            created: number(&v, "created")?,
            keys,
            missing,
            partitions,
        })
    }
}

/// What [`verify`] found in one partition.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PartitionReport {
    /// The partition's origin line.
    pub origin: String,
    /// The checkpoint's tree size.
    pub checkpoint: Option<u64>,
    /// Records proved, present or not.
    pub records: u64,
    /// Parts withheld from the viewer.
    pub withheld: u64,
    /// Parts erased in the log, apart from those withheld.
    pub erased: u64,
    /// Records carried as their leaf alone.
    pub leaf_only: u64,
    /// Offsets left out as past the checkpoint.
    pub pending: u64,
    /// Everything that did not check.
    pub problems: Vec<String>,
}

impl PartitionReport {
    /// Whether nothing failed.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.problems.is_empty()
    }
}

/// Checks an export: the key chain, each checkpoint's signature against it, and each
/// record against its checkpoint by its inclusion proof, its header against its offset,
/// and every withheld part as erased (21 §4.2; 0006 §9). Signatures inside attestations
/// are not checked: the actor key records they need are not carried.
///
/// # Errors
///
/// A key chain that does not verify, against which nothing else can be checked.
pub fn verify(export: &RecordsExport) -> Result<Vec<PartitionReport>, String> {
    let chain = KeyChain::from_records(&export.keys).map_err(|e| e.to_string())?;
    Ok(export
        .partitions
        .iter()
        .map(|p| verify_partition(&chain, p))
        .collect())
}

fn verify_partition(chain: &KeyChain, p: &RecordsPartition) -> PartitionReport {
    let mut r = PartitionReport {
        origin: p.origin.clone(),
        checkpoint: p.checkpoint.as_ref().map(|c| c.checkpoint.size),
        pending: p.pending.len() as u64,
        ..PartitionReport::default()
    };
    let Some(cp) = &p.checkpoint else {
        if !p.entries.is_empty() {
            r.problems
                .push("records with no checkpoint to prove them against".into());
        }
        return r;
    };
    if let Err(e) = cp.verify(&p.origin, &chain.verifiers(&p.origin)) {
        r.problems.push(format!("checkpoint: {e}"));
    }
    let (size, root) = (cp.checkpoint.size, cp.checkpoint.root);
    let mut last: Option<u64> = None;
    for e in &p.entries {
        let at = e.offset;
        if last.is_some_and(|l| l >= at) {
            r.problems.push(format!("{at}: offsets out of order"));
        }
        last = Some(at);
        let path: Option<Vec<Hash>> = e
            .path
            .iter()
            .map(|&i| {
                usize::try_from(i)
                    .ok()
                    .and_then(|i| p.nodes.get(i).copied())
            })
            .collect();
        let Some(path) = path else {
            r.problems
                .push(format!("{at}: a proof names a missing node"));
            continue;
        };
        if let Err(err) = verify_inclusion(&e.shown.leaf(), at, size, &path, &root) {
            r.problems
                .push(format!("{at}: not included in the checkpoint: {err}"));
            continue;
        }
        r.records += 1;
        match &e.shown {
            Shown::Leaf(_) => r.leaf_only += 1,
            Shown::Record { record, withheld } => {
                let h = record.header();
                if h.partition != p.partition || h.offset != at {
                    r.problems.push(format!(
                        "{at}: the header says {}:{}",
                        h.partition, h.offset
                    ));
                }
                for (i, part) in record.body().parts().iter().enumerate() {
                    match (withheld.contains(&i), part.is_erased()) {
                        (true, true) => r.withheld += 1,
                        (true, false) => r
                            .problems
                            .push(format!("{at}: part {i} is listed as withheld but present")),
                        (false, true) => r.erased += 1,
                        (false, false) => {}
                    }
                }
            }
        }
    }
    if p.pending.iter().any(|&o| o < size) {
        r.problems
            .push("an offset the checkpoint covers is listed as pending".into());
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use scatter_log::body::{COMMENT, CONTENT};
    use scatter_log::conformance::draft;
    use scatter_log::genesis::Genesis;
    use scatter_log::header::CONFIG_PARTITION;
    use scatter_log::memory::MemoryStore;
    use scatter_log::registry::{GraphRegistry, KeyEntry, Scope};
    use scatter_log::store::{LogStore, Slot};
    use scatter_log::tree::Segments;

    use crate::checkpoint::tenant_origin;
    use crate::checkpoints::checkpoint;
    use crate::proof::{inclusion_nodes, node_head};

    /// 0006 §9; 21 §4.2: a records export round-trips, verifies with withheld, erased and
    /// leaf-only records, and catches a changed record, a forged leaf and a wrong key.
    #[test]
    #[allow(clippy::too_many_lines)]
    fn a_records_export_verifies() {
        pollster::block_on(async {
            let key = SigningKey::from_bytes(&[33; 32]);
            let mut store = MemoryStore::new();
            Genesis {
                partition: CONFIG_PARTITION,
                scope: Scope::Instance,
                key: KeyEntry::ed25519(key.verifying_key().to_bytes()),
                segment_exponent: 16,
                appended_at: 1,
                founding: Value::map(Vec::new()),
                attestation: Value::map(vec![(
                    Value::text("actor"),
                    Value::text("instance:scatter"),
                )]),
                comment: None,
            }
            .append(&mut store, &GraphRegistry::embedded())
            .await
            .unwrap();
            let keys = vec![store.read(0, 0).await.unwrap().record().unwrap().clone()];
            let origin = tenant_origin("librarybase.org", "pages");
            store
                .create_partition(9, Segments::new(4).unwrap())
                .await
                .unwrap();
            for n in 0..11 {
                store.append(9, draft(n)).await.unwrap();
            }
            store.erase_parts(9, 4, &[COMMENT]).await.unwrap();
            let signed = checkpoint(&store, 9, &origin, &key).await.unwrap();
            store.append(9, draft(11)).await.unwrap();
            let size = signed.checkpoint.size;
            assert_eq!(size, 11);
            let mut leaves = Vec::new();
            for o in 0..size {
                leaves.push(store.read(9, o).await.unwrap().leaf());
            }
            // Offsets 2 (content withheld), 4 (erased comment), 6 (leaf alone), 9.
            let mut nodes: Vec<(u32, u64)> = Vec::new();
            let mut entries = Vec::new();
            for o in [2u64, 4, 6, 9] {
                let path = inclusion_nodes(o, size)
                    .into_iter()
                    .map(|n| {
                        let at = nodes.iter().position(|m| *m == n).unwrap_or_else(|| {
                            nodes.push(n);
                            nodes.len() - 1
                        });
                        at as u64
                    })
                    .collect();
                let Slot::Record(mut record) = store.read(9, o).await.unwrap() else {
                    panic!("present");
                };
                let shown = match o {
                    2 => {
                        record.body_mut().erase(CONTENT).unwrap();
                        Shown::Record {
                            record,
                            withheld: vec![CONTENT],
                        }
                    }
                    6 => Shown::Leaf(record.leaf()),
                    _ => Shown::Record {
                        record,
                        withheld: Vec::new(),
                    },
                };
                entries.push(Entry {
                    offset: o,
                    shown,
                    path,
                });
            }
            let export = RecordsExport {
                created: 7,
                keys: keys.clone(),
                missing: vec!["Nowhere".into()],
                partitions: vec![RecordsPartition {
                    partition: 9,
                    name: "pages".into(),
                    origin: origin.clone(),
                    checkpoint: Some(signed),
                    nodes: nodes
                        .iter()
                        .map(|&(l, i)| node_head(&leaves, l, i, size))
                        .collect(),
                    entries,
                    pending: vec![11],
                }],
            };
            let bytes = export.encode();
            let read = RecordsExport::decode(&bytes).unwrap();
            assert_eq!(read, export);
            let reports = verify(&read).unwrap();
            let r = &reports[0];
            assert!(r.is_clean(), "{:?}", r.problems);
            assert_eq!(
                (r.records, r.withheld, r.erased, r.leaf_only, r.pending),
                (4, 1, 1, 1, 1)
            );
            assert_eq!(r.checkpoint, Some(11));

            // A forged leaf fails its proof.
            let mut forged = read.clone();
            forged.partitions[0].entries[2].shown = Shown::Leaf([7; 32]);
            assert!(!verify(&forged).unwrap()[0].is_clean());
            // A record moved to another offset fails.
            let mut moved = read.clone();
            moved.partitions[0].entries[3].offset = 8;
            assert!(!verify(&moved).unwrap()[0].is_clean());
            // A part said to be withheld must be absent.
            let mut lying = read.clone();
            if let Shown::Record { withheld, .. } = &mut lying.partitions[0].entries[3].shown {
                withheld.push(CONTENT);
            }
            assert!(!verify(&lying).unwrap()[0].is_clean());
            // Another key's checkpoint does not verify against the chain.
            let other = SigningKey::from_bytes(&[34; 32]);
            let mut resigned = read.clone();
            resigned.partitions[0].checkpoint =
                Some(checkpoint(&store, 9, &origin, &other).await.unwrap());
            let problems = &verify(&resigned).unwrap()[0].problems;
            assert!(problems[0].starts_with("checkpoint:"), "{problems:?}");
        });
    }
}
