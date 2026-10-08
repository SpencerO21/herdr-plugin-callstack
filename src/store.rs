use crate::model::{Flow, Record, Scope};
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

pub const INPUT_LIMIT: u64 = 2_000_000;
pub const RECORD_LIMIT: u64 = 4_000_000;
pub fn hash(s: &str) -> String {
    format!("{:x}", Sha256::digest(s.as_bytes()))
}
pub fn read_limited(reader: impl Read, limit: u64) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    reader.take(limit + 1).read_to_end(&mut data)?;
    ensure!(data.len() as u64 <= limit, "Input exceeds {limit} bytes.");
    Ok(data)
}

#[derive(Clone)]
pub struct Store {
    pub directory: PathBuf,
    pub scope: Scope,
}
impl Store {
    pub fn new(root: &Path, scope: Scope) -> Result<Self> {
        let key = serde_json::to_string(&[&scope.namespace, &scope.workspace, &scope.session])?;
        Ok(Self {
            directory: root.join(hash(&key)),
            scope,
        })
    }
    pub fn ensure_dir(&self) -> Result<()> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.directory)?;
        Ok(())
    }
    pub fn file(&self, name: &str) -> PathBuf {
        self.directory.join(format!("{}.json", hash(name)))
    }
    pub fn publish(&self, flow: Flow, project: Option<String>) -> Result<Record> {
        flow.validate()?;
        self.ensure_dir()?;
        let others = self.list();
        let (source_hashes, mut warnings) = crate::audit::publish_checks(
            &flow,
            project.as_deref().map(Path::new),
            others.as_deref().unwrap_or_default(),
        );
        if others.is_err() {
            warnings.push("Could not inspect other saved flows for name conflicts.".into());
        }
        let record = Record {
            archived: self.archive_file(&flow.name).exists(),
            flow,
            scope: self.scope.clone(),
            project_root: project,
            updated_at: OffsetDateTime::now_utc().format(&Rfc3339)?,
            source_hashes,
            warnings,
            drifted_paths: vec![],
        };
        let destination = self.file(&record.flow.name);
        let temporary = destination.with_extension(format!(
            "{}.{}.tmp",
            std::process::id(),
            OffsetDateTime::now_utc().unix_timestamp_nanos()
        ));
        let result = (|| -> Result<()> {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&temporary)?;
            file.write_all(&serde_json::to_vec_pretty(&record)?)?;
            file.sync_all()?;
            fs::rename(&temporary, &destination)?;
            Ok(())
        })();
        let _ = fs::remove_file(&temporary);
        result?;
        Ok(record)
    }
    pub fn delete(&self, name: &str) -> Result<()> {
        fs::remove_file(self.file(name)).context("Flow not found or cannot be removed.")?;
        self.set_archived_marker(name, false)
    }
    fn archive_file(&self, name: &str) -> PathBuf {
        self.file(name).with_extension("archived")
    }
    pub fn set_archived(&self, name: &str, archived: bool) -> Result<()> {
        ensure!(self.file(name).is_file(), "Flow not found.");
        self.set_archived_marker(name, archived)
    }
    fn set_archived_marker(&self, name: &str, archived: bool) -> Result<()> {
        if archived {
            OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .open(self.archive_file(name))?
                .sync_all()?;
        } else if let Err(error) = fs::remove_file(self.archive_file(name))
            && error.kind() != std::io::ErrorKind::NotFound
        {
            return Err(error.into());
        }
        Ok(())
    }
    pub fn list(&self) -> Result<Vec<Arc<Record>>> {
        let mut cache = Cache::default();
        cache.refresh(self)?;
        Ok(crate::audit::SourceCache::default().refresh(&cache.records()))
    }
}

#[derive(PartialEq, Eq, Clone)]
pub(crate) struct Stamp {
    modified: Option<SystemTime>,
    len: u64,
    inode: u64,
    ctime: i64,
    nanos: i64,
}
impl Stamp {
    pub(crate) fn read(path: &Path) -> Result<Self> {
        let m = fs::metadata(path)?;
        Ok(Self {
            modified: m.modified().ok(),
            len: m.len(),
            inode: m.ino(),
            ctime: m.ctime(),
            nanos: m.ctime_nsec(),
        })
    }
}

#[derive(Default)]
pub struct Cache {
    entries: HashMap<PathBuf, (Stamp, Arc<Record>)>,
    pub reads: usize,
}
impl Cache {
    // Called by the file worker, never from mouse/key handling or rendering.
    // Atomic replacement changes the inode even when length and mtime match.
    pub fn refresh(&mut self, store: &Store) -> Result<bool> {
        let entries = match fs::read_dir(&store.directory) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let changed = !self.entries.is_empty();
                self.entries.clear();
                return Ok(changed);
            }
            Err(e) => return Err(e.into()),
        };
        let mut next = HashMap::new();
        let mut changed = false;
        for entry in entries {
            let path = entry?.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let stamp = match Stamp::read(&path) {
                Ok(s) => s,
                Err(_) if !path.exists() => continue,
                Err(e) => return Err(e),
            };
            if let Some((old, record)) = self.entries.get(&path)
                && old == &stamp
            {
                let archived = path.with_extension("archived").exists();
                let mut record = record.clone();
                if archived != record.archived {
                    Arc::make_mut(&mut record).archived = archived;
                    changed = true;
                }
                next.insert(path, (stamp, record));
                continue;
            }
            let file = match File::open(&path) {
                Ok(f) => f,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            };
            let mut record: Record = serde_json::from_slice(&read_limited(file, RECORD_LIMIT)?)
                .with_context(|| format!("Cannot read flow {}", path.display()))?;
            record.flow.validate()?;
            record.archived = path.with_extension("archived").exists();
            ensure!(
                record.scope == store.scope,
                "Flow scope does not match its folder."
            );
            self.reads += 1;
            changed = true;
            next.insert(path, (stamp, Arc::new(record)));
        }
        changed |= next.len() != self.entries.len();
        self.entries = next;
        Ok(changed)
    }
    pub fn records(&self) -> Vec<Arc<Record>> {
        let mut records: Vec<_> = self.entries.values().map(|(_, r)| r.clone()).collect();
        records.sort_by(|a, b| a.flow.name.cmp(&b.flow.name));
        records
    }
}
