//! A tenant's dumps (02 §4.1; 17 §1.1): the resolved view as the Wikibase-compatible dump,
//! and the local graph as the local-graph source dump, each in a plain form and a
//! skolemized one (02 §4.3), written from one exported snapshot.
//!
//! [`dump`] opens a read-only `REPEATABLE READ` transaction on the database it is given (a
//! replica where the operator has one, else the primary; 0032 A12), exports its snapshot,
//! and walks the tenant's entities in that one instant. For each entity an anonymous reader
//! may read it renders:
//!
//! - **`wikibase.nt.gz`**, the resolved view as N-Triples in the shape of
//!   `dumpRdf.php --flavor full-dump`, opened by the `wikibase:Dump` header, with every
//!   mirrored statement that says what its upstream says named as its upstream names it
//!   (0002 A43); and **`wikibase-skolem.nt.gz`**, the same with skolem IRIs for blank nodes;
//! - **`local.nq.gz`** and **`local-skolem.nq.gz`**, the `local` graph's state as N-Quads in
//!   the graph `{base}graph/local`, without the truthy triples a source graph never holds
//!   (05 §1.4); and **`local.json.gz`**, the same states as canonical JSON, one entity per
//!   line, with the coordinates of its newest local record.
//!
//! The files go into `{dir}/.{taken}.partial/` and the directory is renamed to `{dir}/{taken}/`
//! once `manifest.json` is written beside them, so a reader never sees half a dump; the
//! oldest beyond `keep` are then removed. Nothing here serves them.
//!
//! **Not yet:** the full dump, which needs the metadata graph; the update-stream cursor
//! every dump is to be stamped with, which needs the stream (`cursor` is `null` in the
//! manifest); parallel workers sharing the exported snapshot, which the snapshot is
//! exported for.
//!
//! About 0013 §8 (02 §4.1) and 0022 §1 (17 §1.1).

pub mod bundle;
pub mod records;
pub mod xml;

use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::write::GzEncoder;
use oxrdf::{GraphName, NamedNode, NamedOrBlankNode, Quad, Term, Triple};
use scatter_actors::{Effective, ReadDecision, Target as AclTarget, Timestamp, read_decision};
use scatter_normalize::KeyedRegistry;
use scatter_providers::Registry;
use scatter_wikibase_model::{EntityId, Hasher, PageInfo};
use scatter_wikibase_rdf::{
    FORMAT_VERSION, GENID, IriScheme, Namespaces, Produce, Renderer, Seen, dump_header,
};
use serde::Serialize;
use sha2::{Digest as _, Sha256};
use tokio_postgres::{Client, IsolationLevel};
use triplespace_projections::acl_read;
use triplespace_projections::common::Partitions as _;
use triplespace_projections::entities::EntityProjection;
use triplespace_projections::read::{self, add_derived_terms, iso8601};

/// The manifest format.
pub const MANIFEST_FORMAT: u32 = 1;

/// How many entities share one set of written reference and value nodes before it is
/// forgotten, bounding memory: a node met again after that is written again, which
/// N-Triples, being a set, does not mind (Wikibase's dumper bounds its own the same way).
const SEEN_PER: usize = 10_000;

/// What the local-graph source dump writes: every statement node, without the truthy
/// triples a source graph never holds (05 §1.4).
const LOCAL: Produce = Produce::ALL_STATEMENTS
    .union(Produce::QUALIFIERS)
    .union(Produce::REFERENCES)
    .union(Produce::SITELINKS)
    .union(Produce::FULL_VALUES)
    .union(Produce::PAGE_PROPS);

/// What to dump.
pub struct Request<'a> {
    /// The tenant's slug.
    pub tenant: &'a str,
    /// The tenant's registered base, `https://librarybase.org/`.
    pub base: &'a str,
    /// The provider registry with the tenant's entity sources.
    pub registry: &'a Registry,
    /// The provider order the resolved view is composed in.
    pub order: &'a EntityProjection,
    /// What the dump may hold: an anonymous reader's view (17 §5.1).
    pub reader: &'a Effective,
    /// The tenant's dump directory, `{dumps.dir}/{tenant}`.
    pub dir: &'a Path,
    /// How many dumps to keep, the new one included (`dumps.keep`).
    pub keep: usize,
    /// Whether the database read is a replica, recorded in the manifest.
    pub replica: bool,
}

/// One file of a dump, as the manifest lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileEntry {
    /// The file name.
    pub name: String,
    /// `wikibase` or `local`.
    pub product: String,
    /// `plain`, `skolem` or `json`.
    pub form: String,
    /// The media type of the uncompressed content.
    pub media_type: String,
    /// Triples, quads or JSON lines written.
    pub records: u64,
    /// Compressed size.
    pub bytes: u64,
    /// SHA-256 of the compressed file, lowercase hex.
    pub sha256: String,
}

/// The public bundle a dump carries (17 §1.1; 0103 §1): the export scope of the tenant's
/// `local` partition, which a `verified` reader bootstraps from (0022 §2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BundleEntry {
    /// Its directory in the dump, `bundle/`.
    pub path: String,
    /// The partition's checkpoint origin line.
    pub origin: String,
    /// Records in it.
    pub records: u64,
    /// Records it withholds in whole or in part (0006 A30).
    pub withheld: u64,
    /// Checkpoints it carries.
    pub checkpoints: usize,
}

/// A dump's `manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Manifest {
    /// [`MANIFEST_FORMAT`].
    pub format: u32,
    /// The tenant's slug.
    pub tenant: String,
    /// The tenant's base.
    pub base: String,
    /// The instant the snapshot shows, `YYYY-MM-DDTHH:MM:SSZ`; also the dump's directory
    /// name, without punctuation.
    pub taken: String,
    /// The dump's directory name.
    pub name: String,
    /// Whether it was read from a replica.
    pub replica: bool,
    /// Wikibase's RDF format version.
    pub rdf_format: String,
    /// The update-stream cursor the dump continues into; `None` until the stream exists.
    pub cursor: Option<String>,
    /// Entities in the resolved view written.
    pub entities: u64,
    /// Entities with a local-graph state written.
    pub local_entities: u64,
    /// The public bundle of the `local` partition, in `bundle/`, where the tenant has one.
    pub bundle: Option<BundleEntry>,
    /// The files.
    pub files: Vec<FileEntry>,
}

/// Why a dump failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DumpError {
    /// The tenant restricts reading, so it has no public dump (02 §6.4; 09 §4.7).
    #[error("tenant `{0}` is not readable without logging in, so it has no dumps")]
    NotPublic(String),
    /// A database failure.
    #[error("database: {0}")]
    Database(String),
    /// A file failure.
    #[error("{0}: {1}")]
    Io(PathBuf, io::Error),
}

#[allow(clippy::needless_pass_by_value)] // the shape `map_err` hands it
fn db(e: impl ToString) -> DumpError {
    DumpError::Database(e.to_string())
}

/// A writer that hashes and counts what passes through it.
struct Hashed<W> {
    inner: W,
    hash: Sha256,
    bytes: u64,
}

impl<W: Write> Write for Hashed<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hash.update(&buf[..n]);
        self.bytes += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// One gzip file being written.
struct Out {
    entry: FileEntry,
    path: PathBuf,
    gz: GzEncoder<Hashed<BufWriter<File>>>,
}

impl Out {
    fn create(
        dir: &Path,
        name: &str,
        product: &str,
        form: &str,
        media_type: &str,
    ) -> Result<Self, DumpError> {
        let path = dir.join(name);
        let file = File::create(&path).map_err(|e| DumpError::Io(path.clone(), e))?;
        Ok(Self {
            entry: FileEntry {
                name: name.to_string(),
                product: product.to_string(),
                form: form.to_string(),
                media_type: media_type.to_string(),
                records: 0,
                bytes: 0,
                sha256: String::new(),
            },
            gz: GzEncoder::new(
                Hashed {
                    inner: BufWriter::new(file),
                    hash: Sha256::new(),
                    bytes: 0,
                },
                Compression::default(),
            ),
            path,
        })
    }

    fn line(&mut self, line: &str) -> Result<(), DumpError> {
        self.gz
            .write_all(line.as_bytes())
            .and_then(|()| self.gz.write_all(b"\n"))
            .map_err(|e| DumpError::Io(self.path.clone(), e))
    }

    fn record(&mut self, line: &str) -> Result<(), DumpError> {
        self.entry.records += 1;
        self.line(line)
    }

    fn finish(self) -> Result<FileEntry, DumpError> {
        let mut hashed = self
            .gz
            .finish()
            .map_err(|e| DumpError::Io(self.path.clone(), e))?;
        hashed
            .flush()
            .map_err(|e| DumpError::Io(self.path.clone(), e))?;
        let mut entry = self.entry;
        entry.bytes = hashed.bytes;
        entry.sha256 = hashed
            .hash
            .finalize()
            .iter()
            .fold(String::with_capacity(64), |mut s, b| {
                use std::fmt::Write as _;
                let _ = write!(s, "{b:02x}");
                s
            });
        Ok(entry)
    }
}

/// A blank node as the skolem IRI that carries its label (02 §4.3).
fn skolem_node(b: &oxrdf::BlankNode) -> NamedNode {
    NamedNode::new_unchecked(format!("{GENID}{}", b.as_str()))
}

/// The triple with its blank nodes skolemized.
#[must_use]
pub fn skolemize(t: &Triple) -> Triple {
    let subject: NamedOrBlankNode = match &t.subject {
        NamedOrBlankNode::BlankNode(b) => skolem_node(b).into(),
        s @ NamedOrBlankNode::NamedNode(_) => s.clone(),
    };
    let object: Term = match &t.object {
        Term::BlankNode(b) => skolem_node(b).into(),
        o => o.clone(),
    };
    Triple::new(subject, t.predicate.clone(), object)
}

/// Whether `reader` may read the target chain now.
fn readable(reader: &Effective, chain: &[scatter_actors::Acl]) -> bool {
    read_decision(reader, chain, Timestamp(now_micros())) == ReadDecision::Readable
}

fn now_micros() -> u64 {
    u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_micros()),
    )
    .unwrap_or(u64::MAX)
}

/// Writes a dump of the tenant. See the crate's documentation.
///
/// # Errors
///
/// [`DumpError::NotPublic`] for a tenant an anonymous reader may not read; a database or
/// file failure otherwise, after which no dump directory is left.
pub async fn dump(client: &mut Client, req: &Request<'_>) -> Result<Manifest, DumpError> {
    let tx = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await
        .map_err(db)?;
    let row = tx
        .query_one(
            "SELECT pg_export_snapshot(),
                    to_char(now() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'),
                    to_char(now() AT TIME ZONE 'UTC', 'YYYYMMDD\"T\"HH24MISS\"Z\"')",
            &[],
        )
        .await
        .map_err(db)?;
    let taken: String = row.get(1);
    let name: String = row.get(2);

    let tenant_chain = acl_read::current(
        &tx,
        req.tenant,
        &[AclTarget::Tenant(req.tenant.to_string())],
    )
    .await
    .map_err(db)?;
    if !readable(req.reader, &tenant_chain) {
        return Err(DumpError::NotPublic(req.tenant.to_string()));
    }

    fs::create_dir_all(req.dir).map_err(|e| DumpError::Io(req.dir.to_path_buf(), e))?;
    let partial = req.dir.join(format!(".{name}.partial"));
    if partial.exists() {
        fs::remove_dir_all(&partial).map_err(|e| DumpError::Io(partial.clone(), e))?;
    }
    fs::create_dir_all(&partial).map_err(|e| DumpError::Io(partial.clone(), e))?;

    let result = match write_files(&tx, req, &partial, &taken, &name).await {
        Ok(mut m) => match write_bundle(&tx, req, &partial.join("bundle")).await {
            Ok(b) => {
                m.bundle = b;
                Ok(m)
            }
            Err(e) => Err(e),
        },
        Err(e) => Err(e),
    };
    let manifest = match result {
        Ok(m) => m,
        Err(e) => {
            let _ = fs::remove_dir_all(&partial);
            return Err(e);
        }
    };
    tx.commit().await.map_err(db)?;

    let json = serde_json::to_vec_pretty(&manifest).unwrap_or_default();
    let manifest_path = partial.join("manifest.json");
    fs::write(&manifest_path, json).map_err(|e| DumpError::Io(manifest_path, e))?;
    let final_dir = req.dir.join(&name);
    if final_dir.exists() {
        fs::remove_dir_all(&final_dir).map_err(|e| DumpError::Io(final_dir.clone(), e))?;
    }
    fs::rename(&partial, &final_dir).map_err(|e| DumpError::Io(final_dir.clone(), e))?;
    prune(req.dir, req.keep)?;
    Ok(manifest)
}

/// The dump names in `dir`, oldest first: the directories a [`dump`] completed.
///
/// # Errors
///
/// A failure to read the directory; a directory that does not exist has no dumps.
pub fn list(dir: &Path) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(names),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if entry.file_type()?.is_dir()
            && is_dump_name(&name)
            && entry.path().join("manifest.json").exists()
        {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

/// Whether `name` is a dump directory's name, `YYYYMMDDTHHMMSSZ`.
#[must_use]
pub fn is_dump_name(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() == 16
        && b[8] == b'T'
        && b[15] == b'Z'
        && b[..8].iter().chain(&b[9..15]).all(u8::is_ascii_digit)
}

fn prune(dir: &Path, keep: usize) -> Result<(), DumpError> {
    let names = list(dir).map_err(|e| DumpError::Io(dir.to_path_buf(), e))?;
    let excess = names.len().saturating_sub(keep.max(1));
    for name in &names[..excess] {
        let path = dir.join(name);
        fs::remove_dir_all(&path).map_err(|e| DumpError::Io(path, e))?;
    }
    Ok(())
}

/// The five files of a dump.
struct Files {
    wikibase: Out,
    wikibase_skolem: Out,
    local: Out,
    local_skolem: Out,
    local_json: Out,
}

impl Files {
    fn create(dir: &Path) -> Result<Self, DumpError> {
        let nt = "application/n-triples";
        let nq = "application/n-quads";
        Ok(Self {
            wikibase: Out::create(dir, "wikibase.nt.gz", "wikibase", "plain", nt)?,
            wikibase_skolem: Out::create(dir, "wikibase-skolem.nt.gz", "wikibase", "skolem", nt)?,
            local: Out::create(dir, "local.nq.gz", "local", "plain", nq)?,
            local_skolem: Out::create(dir, "local-skolem.nq.gz", "local", "skolem", nq)?,
            local_json: Out::create(
                dir,
                "local.json.gz",
                "local",
                "json",
                "application/x-ndjson",
            )?,
        })
    }

    /// The first line of every RDF file, and the dump header of the resolved view's.
    fn open(&mut self, base: &str, taken: &str) -> Result<(), DumpError> {
        // Every dump is stamped on its first line; the cursor comes with the update stream.
        let stamp =
            format!("# Triplespace dump of {base} taken {taken}; update-stream cursor: none");
        for out in [
            &mut self.wikibase,
            &mut self.wikibase_skolem,
            &mut self.local,
            &mut self.local_skolem,
        ] {
            out.line(&stamp)?;
        }
        self.resolved(&dump_header(taken))
    }

    /// Triples of the resolved view, in both forms.
    fn resolved(&mut self, triples: &[Triple]) -> Result<(), DumpError> {
        for t in triples {
            self.wikibase.record(&format!("{t} ."))?;
            self.wikibase_skolem
                .record(&format!("{} .", skolemize(t)))?;
        }
        Ok(())
    }

    /// Triples of the local graph, as quads in `graph`, in both forms.
    fn local(&mut self, triples: &[Triple], graph: &GraphName) -> Result<(), DumpError> {
        for t in triples {
            let quad = Quad::new(
                t.subject.clone(),
                t.predicate.clone(),
                t.object.clone(),
                graph.clone(),
            );
            self.local.record(&format!("{quad} ."))?;
            let s = skolemize(t);
            let quad = Quad::new(s.subject, s.predicate, s.object, graph.clone());
            self.local_skolem.record(&format!("{quad} ."))?;
        }
        Ok(())
    }

    fn finish(self) -> Result<Vec<FileEntry>, DumpError> {
        Ok(vec![
            self.wikibase.finish()?,
            self.wikibase_skolem.finish()?,
            self.local.finish()?,
            self.local_skolem.finish()?,
            self.local_json.finish()?,
        ])
    }
}

/// What every entity of one dump is written with.
struct Walk<'a> {
    tx: &'a tokio_postgres::Transaction<'a>,
    req: &'a Request<'a>,
    iris: IriScheme<'a>,
    hasher: Hasher,
    local_graph: GraphName,
    local_partition: Option<u64>,
}

/// What one entity added to the dump.
#[derive(Default)]
struct Wrote {
    resolved: bool,
    local: bool,
}

impl Walk<'_> {
    /// One entity: its resolved view, and its local graph's state where it has one, each
    /// where an anonymous reader may read it.
    async fn entity(
        &self,
        id: &EntityId,
        files: &mut Files,
        seen: &mut Seen,
        local_seen: &mut Seen,
    ) -> Result<Wrote, DumpError> {
        let (tx, req) = (self.tx, self.req);
        let mut wrote = Wrote::default();
        let Some(current) = read::current(tx, req.order, req.tenant, id)
            .await
            .map_err(db)?
        else {
            return Ok(wrote);
        };
        let serving = current.contributions.first().map(|c| c.graph.clone());
        let chain = acl_read::entity_chain(tx, req.tenant, id.as_str(), serving.as_deref())
            .await
            .map_err(db)?;
        if !readable(req.reader, &chain) {
            return Ok(wrote);
        }

        // The resolved view.
        let mut entity = current.resolved.entity.clone();
        add_derived_terms(&mut entity);
        let page = PageInfo {
            lastrevid: current.lastrevid,
            modified: Some(iso8601(current.modified)),
            ..PageInfo::default()
        };
        let upstream = current.upstream_statements(req.registry);
        let mut triples = Vec::new();
        Renderer::new(&self.iris, &self.hasher, Produce::DUMP_FILE)
            .with_upstream(&upstream)
            .entity_into(&entity, Some(&page), seen, &mut triples);
        files.resolved(&triples)?;
        wrote.resolved = true;

        // The local graph's own state, with its record coordinates.
        let Some(local) = current.local() else {
            return Ok(wrote);
        };
        let chain = acl_read::entity_chain(tx, req.tenant, id.as_str(), Some("local"))
            .await
            .map_err(db)?;
        if !readable(req.reader, &chain) {
            return Ok(wrote);
        }
        let page = PageInfo {
            lastrevid: current.local_revid,
            ..PageInfo::default()
        };
        let mut triples = Vec::new();
        Renderer::new(&self.iris, &self.hasher, LOCAL).entity_into(
            local,
            Some(&page),
            local_seen,
            &mut triples,
        );
        files.local(&triples, &self.local_graph)?;
        let line = serde_json::json!({
            "id": local.id.as_str(),
            "partition": self.local_partition,
            "offset": current.local_offset,
            "revid": current.local_revid,
            "entity": serde_json::from_str::<serde_json::Value>(&local.to_canonical_json())
                .unwrap_or(serde_json::Value::Null),
        });
        files.local_json.record(&line.to_string())?;
        wrote.local = true;
        Ok(wrote)
    }
}

/// Writes the export-scope bundle of the tenant's `local` partition into `dest`, in the
/// dump's own snapshot (0013 A81): what the reader may read of it, the rest withheld and
/// listed (0006 A30, A31). `None` when the tenant has no `local` partition yet.
async fn write_bundle(
    tx: &tokio_postgres::Transaction<'_>,
    req: &Request<'_>,
    dest: &Path,
) -> Result<Option<BundleEntry>, DumpError> {
    use scatter_integrity::bundle::{BundlePartition, export_scoped};
    use scatter_log_postgres::checkpoints::PgCheckpoints;
    use scatter_log_postgres::log::PgLog;
    let rows = bundle::partition_rows(tx, Some(req.tenant))
        .await
        .map_err(DumpError::Database)?;
    let Some(row) = rows
        .into_iter()
        .find(|r| r.name == "local" && r.export == "public")
    else {
        return Ok(None);
    };
    let reader = bundle::Reader::new(tx, req.tenant, req.reader.clone())
        .await
        .map_err(DumpError::Database)?;
    if reader.viewer.hidden.graphs.contains(&row.name) {
        return Ok(None);
    }
    let store = PgLog::new(tx);
    let checkpoints = PgCheckpoints::new(tx);
    let origin = bundle::origin_of(tx, &row, "")
        .await
        .map_err(DumpError::Database)?;
    let ids = [scatter_log_postgres::ids::partition_to_db(row.partition)];
    let withhold = reader
        .decide(tx, req.tenant, &ids, bundle::Offsets::All)
        .await
        .map_err(DumpError::Database)?;
    let keys = bundle::key_records(tx).await.map_err(DumpError::Database)?;
    let partition = BundlePartition {
        partition: row.partition,
        name: row.name.clone(),
        origin: origin.clone(),
    };
    export_scoped(
        &store,
        &checkpoints,
        std::slice::from_ref(&partition),
        &keys,
        now_micros(),
        dest,
        "export",
        &withhold,
    )
    .await
    .map_err(|e| DumpError::Io(dest.to_path_buf(), io::Error::other(e.to_string())))?;
    let opened = scatter_integrity::bundle::Bundle::open(dest)
        .map_err(|e| DumpError::Io(dest.to_path_buf(), io::Error::other(e.to_string())))?;
    let report = opened
        .verify(scatter_integrity::verify::Level::Structure, None, None)
        .await
        .remove(0);
    Ok(Some(BundleEntry {
        path: "bundle/".into(),
        origin,
        records: report.records,
        withheld: opened
            .withheld()
            .get(&row.partition)
            .map_or(0, |l| l.len() as u64),
        checkpoints: report.checkpoints,
    }))
}

async fn write_files(
    tx: &tokio_postgres::Transaction<'_>,
    req: &Request<'_>,
    dir: &Path,
    taken: &str,
    name: &str,
) -> Result<Manifest, DumpError> {
    let iris = IriScheme::new(
        Namespaces::triplespace(req.base),
        req.registry,
        KeyedRegistry::default_registry(),
    );
    let base = iris.namespaces().base.clone();
    let walk = Walk {
        tx,
        req,
        local_graph: GraphName::NamedNode(NamedNode::new_unchecked(format!("{base}graph/local"))),
        local_partition: tx.partition_of(req.tenant, "local").await.map_err(db)?,
        iris,
        hasher: Hasher::local(),
    };
    let mut files = Files::create(dir)?;
    files.open(&base, taken)?;

    let ids: Vec<String> = tx
        .query(
            "SELECT DISTINCT id FROM view.entity WHERE tenant IN ($1, '') AND NOT deleted ORDER BY id",
            &[&req.tenant],
        )
        .await
        .map_err(db)?
        .into_iter()
        .map(|r| r.get(0))
        .collect();
    let (mut entities, mut local_entities) = (0u64, 0u64);
    let (mut seen, mut local_seen) = (Seen::default(), Seen::default());
    for (i, text) in ids.iter().enumerate() {
        if i % SEEN_PER == 0 {
            seen = Seen::default();
            local_seen = Seen::default();
        }
        let Ok(id) = EntityId::parse(text) else {
            continue;
        };
        let wrote = walk
            .entity(&id, &mut files, &mut seen, &mut local_seen)
            .await?;
        entities += u64::from(wrote.resolved);
        local_entities += u64::from(wrote.local);
    }

    Ok(Manifest {
        format: MANIFEST_FORMAT,
        tenant: req.tenant.to_string(),
        base,
        taken: taken.to_string(),
        name: name.to_string(),
        replica: req.replica,
        rdf_format: FORMAT_VERSION.to_string(),
        cursor: None,
        entities,
        local_entities,
        bundle: None,
        files: files.finish()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dump_names() {
        assert!(is_dump_name("20261107T231553Z"));
        assert!(!is_dump_name(".20261107T231553Z.partial"));
        assert!(!is_dump_name("latest"));
        assert!(!is_dump_name("local"));
    }

    #[test]
    fn skolemizes_blank_nodes_only() {
        let t = Triple::new(
            oxrdf::BlankNode::new_unchecked("ab12"),
            NamedNode::new_unchecked("http://example.org/p"),
            oxrdf::BlankNode::new_unchecked("cd34"),
        );
        let s = skolemize(&t);
        assert_eq!(
            s.to_string(),
            "<https://scatter.red/genid/ab12> <http://example.org/p> <https://scatter.red/genid/cd34>"
        );
    }
}
