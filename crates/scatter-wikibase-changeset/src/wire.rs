//! The NDJSON wire format (0002 §8.7; payloads.md §3): a job line, then one operation
//! per line. Operations are read as raw JSON first, because a batch may name entities it
//! creates by temporary handles (`$w1`, 0002 §8.5) that the server replaces with minted
//! IDs before the typed [`Operation`] exists; [`Batch::resolve_refs`] does the rewrite in
//! every ID position and nowhere else, so a label that happens to start with `$` is left
//! alone.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, Write};

use scatter_wikibase_model::id::EntityId;
use serde_json::{Map, Value};

use crate::job::JobHeader;
use crate::op::Operation;

/// Why a batch could not be read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum WireError {
    /// Reading failed.
    #[error("read: {0}")]
    Io(#[from] std::io::Error),
    /// No lines at all.
    #[error("the batch is empty")]
    Empty,
    /// A line is not JSON, or not the JSON the position needs.
    #[error("line {line}: {source}")]
    Json {
        /// 1-based.
        line: usize,
        /// What serde said.
        #[source]
        source: serde_json::Error,
    },
    /// The first line is not `{"job": {...}}`.
    #[error("line 1: the first line is the job line, `{{\"job\": {{...}}}}`")]
    NoJob,
    /// A line is not an object with an `op`.
    #[error("line {line}: an operation is an object with `op`")]
    NotAnOperation {
        /// 1-based.
        line: usize,
    },
    /// Two operations declare the same handle.
    #[error("line {line}: `{handle}` is declared twice")]
    DuplicateRef {
        /// 1-based.
        line: usize,
        /// The handle.
        handle: String,
    },
    /// A handle is used that no `create` or `create-or-add` in the batch declares.
    #[error("line {line}: `{handle}` is not declared by any `ref` in this batch")]
    UndeclaredRef {
        /// 1-based.
        line: usize,
        /// The handle.
        handle: String,
    },
    /// A declared handle was given no ID.
    #[error("line {line}: no ID was minted for `{handle}`")]
    Unresolved {
        /// 1-based.
        line: usize,
        /// The handle.
        handle: String,
    },
    /// A handle is not `$` followed by letters, digits or `_`.
    #[error("line {line}: `{handle}` is not a handle (`$` then letters, digits or `_`)")]
    BadHandle {
        /// 1-based.
        line: usize,
        /// The text.
        handle: String,
    },
}

/// Whether a string is a temporary handle: `$` then one or more of `[A-Za-z0-9_]`.
#[must_use]
pub fn is_handle(s: &str) -> bool {
    s.len() > 1
        && s.starts_with('$')
        && s[1..]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// One operation as read: its line and the raw object.
#[derive(Debug, Clone, PartialEq)]
pub struct WireOp {
    /// 1-based line number in the batch, the job line being 1.
    pub line: usize,
    /// The object.
    pub value: Map<String, Value>,
}

/// Fields holding one entity ID, and arrays of them, where a handle may stand.
const ID_FIELDS: &[&str] = &["id", "to", "from", "local", "property"];
const ID_ARRAYS: &[&str] = &["ids", "badges"];
/// Objects keyed by property ID.
const PROPERTY_MAPS: &[&str] = &["claims", "statements", "qualifiers", "snaks"];

impl WireOp {
    /// The `op` name.
    #[must_use]
    pub fn op(&self) -> Option<&str> {
        self.value.get("op").and_then(Value::as_str)
    }

    /// The handle this operation declares with `ref`.
    #[must_use]
    pub fn declared_ref(&self) -> Option<&str> {
        self.value.get("ref").and_then(Value::as_str)
    }

    /// Every handle used in an ID position, `ref` itself excluded.
    #[must_use]
    pub fn used_refs(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for (k, v) in &self.value {
            if k != "ref" {
                collect(k, v, &mut out);
            }
        }
        out
    }

    /// Gives a `create` or `create-or-add` its ID: drops `ref`, sets `id` on a `create`,
    /// and sets the `entity`'s `id`, whatever placeholder it carried. For a
    /// `create-or-add` the ID is the matched entity's or the minted one, as the server
    /// resolved it.
    pub fn assign_id(&mut self, id: &EntityId) {
        self.value.remove("ref");
        let text = Value::String(id.as_str().to_string());
        if self.op() == Some("create") {
            self.value.insert("id".to_string(), text.clone());
        }
        if let Some(Value::Object(entity)) = self.value.get_mut("entity") {
            entity.insert("id".to_string(), text);
        }
    }

    /// Replaces every handle in an ID position with its minted ID, and gives an
    /// operation with a declared `ref` that handle's ID ([`WireOp::assign_id`]). Handles
    /// not in `ids` are an error.
    pub fn resolve_refs(&mut self, ids: &BTreeMap<String, EntityId>) -> Result<(), WireError> {
        let line = self.line;
        if let Some(handle) = self.declared_ref().map(str::to_owned) {
            let id = ids.get(&handle).ok_or_else(|| WireError::Unresolved {
                line,
                handle: handle.clone(),
            })?;
            self.assign_id(id);
        }
        let mut object = std::mem::take(&mut self.value);
        let result = rewrite_object(&mut object, ids, line);
        self.value = object;
        result
    }

    /// The typed operation. Fails on a shape error, or on a handle still in place.
    pub fn typed(&self) -> Result<Operation, WireError> {
        if let Some(h) = self.used_refs().into_iter().next() {
            return Err(WireError::Unresolved {
                line: self.line,
                handle: h,
            });
        }
        serde_json::from_value(Value::Object(self.value.clone())).map_err(|source| {
            WireError::Json {
                line: self.line,
                source,
            }
        })
    }
}

fn collect(key: &str, v: &Value, out: &mut BTreeSet<String>) {
    match v {
        Value::String(s) if ID_FIELDS.contains(&key) && is_handle(s) => {
            out.insert(s.clone());
        }
        Value::Array(items) if ID_ARRAYS.contains(&key) => {
            for s in items
                .iter()
                .filter_map(Value::as_str)
                .filter(|s| is_handle(s))
            {
                out.insert(s.to_string());
            }
        }
        Value::Array(items) => {
            for item in items {
                collect("", item, out);
            }
        }
        Value::Object(m) => {
            if PROPERTY_MAPS.contains(&key) {
                for k in m.keys().filter(|k| is_handle(k)) {
                    out.insert(k.clone());
                }
            }
            if key == "match" {
                for k in m.keys().filter(|k| is_handle(k)) {
                    out.insert(k.clone());
                }
            }
            for (k, v) in m {
                collect(k, v, out);
            }
        }
        _ => {}
    }
}

fn rewrite_object(
    m: &mut Map<String, Value>,
    ids: &BTreeMap<String, EntityId>,
    line: usize,
) -> Result<(), WireError> {
    let keys: Vec<String> = m.keys().cloned().collect();
    for k in keys {
        let mut v = m.remove(&k).expect("present");
        rewrite(&k, &mut v, ids, line)?;
        m.insert(k, v);
    }
    Ok(())
}

fn rewrite_string(
    s: &mut String,
    ids: &BTreeMap<String, EntityId>,
    line: usize,
) -> Result<(), WireError> {
    if is_handle(s) {
        let id = ids
            .get(s.as_str())
            .ok_or_else(|| WireError::UndeclaredRef {
                line,
                handle: s.clone(),
            })?;
        *s = id.as_str().to_string();
    }
    Ok(())
}

fn rewrite(
    key: &str,
    v: &mut Value,
    ids: &BTreeMap<String, EntityId>,
    line: usize,
) -> Result<(), WireError> {
    match v {
        Value::String(s) if ID_FIELDS.contains(&key) => rewrite_string(s, ids, line),
        Value::Array(items) => {
            for item in items {
                if ID_ARRAYS.contains(&key) {
                    if let Value::String(s) = item {
                        rewrite_string(s, ids, line)?;
                    }
                } else {
                    rewrite("", item, ids, line)?;
                }
            }
            Ok(())
        }
        Value::Object(m) => {
            if PROPERTY_MAPS.contains(&key) || key == "match" {
                let keys: Vec<String> = m.keys().cloned().collect();
                for k in keys {
                    let mut nk = k.clone();
                    rewrite_string(&mut nk, ids, line)?;
                    if nk != k {
                        let v = m.remove(&k).expect("present");
                        m.insert(nk, v);
                    }
                }
            }
            rewrite_object(m, ids, line)
        }
        _ => Ok(()),
    }
}

/// A read batch: the job line and the operations as raw objects.
#[derive(Debug, Clone, PartialEq)]
pub struct Batch {
    /// The job line.
    pub job: JobHeader,
    /// The operations, in order.
    pub ops: Vec<WireOp>,
}

impl Batch {
    /// Reads a batch from text.
    pub fn parse(text: &str) -> Result<Self, WireError> {
        Self::read(text.as_bytes())
    }

    /// Reads a batch line by line. Blank lines are skipped but still counted.
    pub fn read<R: BufRead>(reader: R) -> Result<Self, WireError> {
        let mut job = None;
        let mut ops = Vec::new();
        for (i, line) in reader.lines().enumerate() {
            let line = line?;
            let n = i + 1;
            if line.trim().is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(&line)
                .map_err(|source| WireError::Json { line: n, source })?;
            let Value::Object(mut m) = value else {
                return Err(if job.is_none() {
                    WireError::NoJob
                } else {
                    WireError::NotAnOperation { line: n }
                });
            };
            if job.is_none() {
                let Some(j) = m.remove("job") else {
                    return Err(WireError::NoJob);
                };
                if !m.is_empty() {
                    return Err(WireError::NoJob);
                }
                job = Some(
                    serde_json::from_value(j)
                        .map_err(|source| WireError::Json { line: n, source })?,
                );
                continue;
            }
            if !m.get("op").is_some_and(Value::is_string) {
                return Err(WireError::NotAnOperation { line: n });
            }
            ops.push(WireOp { line: n, value: m });
        }
        let job = job.ok_or(WireError::Empty)?;
        Ok(Self { job, ops })
    }

    /// The handles the batch declares, in order, after checking that each is declared
    /// once and well formed and that every used handle is declared.
    pub fn refs(&self) -> Result<Vec<String>, WireError> {
        let mut declared: Vec<String> = Vec::new();
        for op in &self.ops {
            if let Some(h) = op.declared_ref() {
                if !is_handle(h) {
                    return Err(WireError::BadHandle {
                        line: op.line,
                        handle: h.to_string(),
                    });
                }
                if declared.iter().any(|d| d == h) {
                    return Err(WireError::DuplicateRef {
                        line: op.line,
                        handle: h.to_string(),
                    });
                }
                declared.push(h.to_string());
            }
        }
        for op in &self.ops {
            for used in op.used_refs() {
                if !declared.contains(&used) {
                    return Err(WireError::UndeclaredRef {
                        line: op.line,
                        handle: used,
                    });
                }
            }
        }
        Ok(declared)
    }

    /// Replaces every handle with its minted ID.
    pub fn resolve_refs(&mut self, ids: &BTreeMap<String, EntityId>) -> Result<(), WireError> {
        for op in &mut self.ops {
            op.resolve_refs(ids)?;
        }
        Ok(())
    }

    /// Every operation, typed. A batch with handles is resolved first.
    pub fn operations(&self) -> Result<Vec<Operation>, WireError> {
        self.ops.iter().map(WireOp::typed).collect()
    }

    /// Writes a batch: the job line, then one operation per line.
    pub fn write<'a, W: Write>(
        mut w: W,
        job: &JobHeader,
        ops: impl IntoIterator<Item = &'a Operation>,
    ) -> std::io::Result<()> {
        let mut first = Map::new();
        first.insert(
            "job".to_string(),
            serde_json::to_value(job).map_err(std::io::Error::other)?,
        );
        serde_json::to_writer(&mut w, &Value::Object(first))?;
        w.write_all(b"\n")?;
        for op in ops {
            serde_json::to_writer(&mut w, op)?;
            w.write_all(b"\n")?;
        }
        Ok(())
    }
}
