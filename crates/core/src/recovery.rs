//! Offline diagnosis: copy evidence under the owner's lock, then let SQLite
//! recover ONLY a separate catalog copy. Never call Volume::open or run GC.
//! Requires a trusted, exclusively controlled source and workspace namespace.
use crate::{BLOCK_SIZE, Error, FORMAT, Result, names, owner::OwnerLock, segment};
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Copy, Debug)]
pub struct RecoveryLimits {
    pub snapshot_bytes: u64,
    pub catalog_rows: u64,
    pub depth: usize,
    pub export_bytes: u64,
}
impl Default for RecoveryLimits {
    fn default() -> Self {
        Self {
            snapshot_bytes: 64 << 30,
            catalog_rows: 1_000_000,
            depth: 256,
            export_bytes: 64 << 30,
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct RecoveryReport {
    pub files: u64,
    pub directories: u64,
    pub orphan_files: u64,
    pub blocks_checked: u64,
    pub logical_bytes: u64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExportLinks {
    /// Fail rather than silently copy if the target cannot preserve hard links.
    Preserve,
    /// Explicit recovery mode, NOT a hard-link implementation or certification.
    CopyAliases,
}
#[derive(Clone, Debug, Default)]
pub struct ExportReport {
    pub files: u64,
    pub directories: u64,
    pub hard_links: u64,
    pub copied_aliases: u64,
    pub bytes_written: u64,
}
#[derive(Clone, Debug)]
struct Object {
    kind: i64,
    size: u64,
}
#[derive(Clone, Debug)]
struct Item {
    parent: i64,
    object: i64,
    name: String,
}
struct Catalog {
    objects: BTreeMap<i64, Object>,
    items: Vec<Item>,
    report: RecoveryReport,
}

/// Owns a recovered *working* catalog; original evidence stays unopened by SQLite.
/// Dropping this object closes the catalog but intentionally preserves the workspace.
pub struct Recovery {
    db: Connection,
    source: PathBuf,
    workspace: PathBuf,
    evidence: PathBuf,
    limits: RecoveryLimits,
}

fn invalid(message: &str) -> Error {
    Error::Corrupt(message.into())
}
fn bounded(value: u64, limit: u64) -> Result<()> {
    if value > limit {
        return Err(Error::RecoveryLimit);
    }
    Ok(())
}
fn new_path(path: &Path) -> Result<PathBuf> {
    let name = match path.components().next_back() {
        Some(Component::Normal(name)) => name,
        _ => return Err(Error::InvalidName),
    };
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    Ok(fs::canonicalize(parent)?.join(name))
}
fn copy_file(source: &Path, target: &Path, remaining: &mut u64) -> Result<(u64, String)> {
    segment::regular(source)?;
    copy_open_file(&File::open(source)?, target, remaining)
}
fn copy_open_file(mut input: &File, target: &Path, remaining: &mut u64) -> Result<(u64, String)> {
    let expected = input.metadata()?.len();
    bounded(expected, *remaining)?;
    let mut output = File::create_new(target)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; BLOCK_SIZE];
    let mut copied = 0_u64;
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        *remaining = remaining
            .checked_sub(n as u64)
            .ok_or(Error::RecoveryLimit)?;
        copied = copied.checked_add(n as u64).ok_or(Error::RecoveryLimit)?;
        output.write_all(&buffer[..n])?;
        digest.update(&buffer[..n]);
    }
    if copied != expected {
        return Err(invalid("source changed during snapshot"));
    }
    output.sync_all()?;
    Ok((copied, format!("{:x}", digest.finalize())))
}

impl Recovery {
    /// `workspace` must not exist. Failed snapshots are left visibly incomplete.
    /// Reading may update host access timestamps; this is not a hardware write blocker.
    pub fn prepare(
        source: impl AsRef<Path>,
        workspace: impl AsRef<Path>,
        limits: RecoveryLimits,
    ) -> Result<Self> {
        if rusqlite::version_number() < 3_051_003 {
            return Err(Error::UnsupportedFormat);
        }
        if !fs::symlink_metadata(source.as_ref())?.file_type().is_dir() {
            return Err(Error::UnsupportedFormat);
        }
        let source = fs::canonicalize(source)?;
        let workspace = new_path(workspace.as_ref())?;
        if workspace.starts_with(&source) {
            return Err(Error::InvalidRange);
        }
        segment::regular(&source.join("session.lock"))?;
        // Read-only descriptor: no creation, timestamp writes or stale-lock stealing.
        let owner = OwnerLock::acquire(File::open(source.join("session.lock"))?)?;
        for name in ["format.txt", "catalog.sqlite"] {
            segment::regular(&source.join(name))?;
        }
        if fs::metadata(source.join("format.txt"))?.len() > 128
            || fs::read(source.join("format.txt"))? != FORMAT.as_bytes()
            || !fs::symlink_metadata(source.join("segments"))?
                .file_type()
                .is_dir()
        {
            return Err(Error::UnsupportedFormat);
        }
        fs::create_dir(&workspace)?;
        File::create_new(workspace.join("SNAPSHOT_INCOMPLETE"))?.sync_all()?;
        let evidence = workspace.join("evidence");
        fs::create_dir(&evidence)?;
        fs::create_dir(evidence.join("segments"))?;
        let mut manifest = File::create_new(workspace.join("snapshot-manifest.txt"))?;
        writeln!(
            manifest,
            "PortaWeave evidence snapshot v1\nsource={source:?}\nsha256 bytes relative_path"
        )?;
        let mut remaining = limits.snapshot_bytes;
        let mut count = 0_u64;
        for directory in [Path::new(""), Path::new("segments")] {
            for entry in fs::read_dir(source.join(directory))? {
                let entry = entry?;
                let relative = directory.join(entry.file_name());
                if relative == Path::new("segments") {
                    continue;
                }
                count = count.checked_add(1).ok_or(Error::RecoveryLimit)?;
                bounded(count, limits.catalog_rows)?;
                // Unknown regular files are preserved; links/devices/nested trees are refused.
                let target = evidence.join(&relative);
                let (bytes, hash) = if relative == Path::new("session.lock") {
                    // Windows rejects reading a locked range through a second handle.
                    // Keep the lock and copy through the exact handle which owns it.
                    copy_open_file(owner.file(), &target, &mut remaining)?
                } else {
                    copy_file(&entry.path(), &target, &mut remaining)?
                };
                writeln!(manifest, "{hash} {bytes} {relative:?}")?;
            }
        }
        manifest.sync_all()?;
        segment::sync_directory(&evidence.join("segments"))?;
        segment::sync_directory(&evidence)?;
        File::create_new(workspace.join("SNAPSHOT_COMPLETE"))?.sync_all()?;
        fs::remove_file(workspace.join("SNAPSHOT_INCOMPLETE"))?;
        segment::sync_directory(&workspace)?;
        drop(owner); // The frozen, independently copied evidence is now sufficient.

        let working = workspace.join("catalog-work");
        fs::create_dir(&working)?;
        let mut working_budget = limits.snapshot_bytes;
        for name in [
            "catalog.sqlite",
            "catalog.sqlite-wal",
            "catalog.sqlite-journal",
        ] {
            let path = evidence.join(name);
            if path.try_exists()? {
                copy_file(&path, &working.join(name), &mut working_budget)?;
            }
        }
        // Never copy the WAL index into the working view: SQLite rebuilds it from
        // the preserved WAL. Opening READ_WRITE is confined to this disposable copy.
        let db = Connection::open_with_flags(
            working.join("catalog.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_WRITE,
        )?;
        db.busy_timeout(std::time::Duration::ZERO)?;
        db.execute_batch(
            "PRAGMA trusted_schema=OFF; PRAGMA foreign_keys=ON; PRAGMA query_only=ON;",
        )?;
        Ok(Self {
            db,
            source,
            workspace,
            evidence,
            limits,
        })
    }
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    fn catalog(&self) -> Result<Catalog> {
        let app: i64 = self
            .db
            .query_row("PRAGMA application_id", [], |r| r.get(0))?;
        let version: i64 = self.db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if app != 1347898693 || version != 1 {
            return Err(Error::UnsupportedFormat);
        }
        crate::catalog::validate_schema(&self.db)?;
        let checks = self
            .db
            .prepare("PRAGMA quick_check")?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if checks != ["ok"] {
            return Err(invalid("SQLite integrity check failed"));
        }
        if self.db.prepare("PRAGMA foreign_key_check")?.exists([])? {
            return Err(invalid("foreign key violation"));
        }
        let (format, quota): (String, i64) =
            self.db
                .query_row("SELECT format,quota FROM config WHERE id=1", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?;
        if format != FORMAT || quota < 0 {
            return Err(Error::UnsupportedFormat);
        }
        let mut rows = 0_u64;
        for table in ["objects", "entries", "blocks", "segments"] {
            let count: i64 =
                self.db
                    .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?;
            rows = rows
                .checked_add(u64::try_from(count).map_err(|_| Error::RecoveryLimit)?)
                .ok_or(Error::RecoveryLimit)?;
            bounded(rows, self.limits.catalog_rows)?;
        }
        let mut objects = BTreeMap::new();
        let mut report = RecoveryReport::default();
        let mut statement = self
            .db
            .prepare("SELECT id,kind,size,generation FROM objects")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let (id, kind, size, generation): (i64, i64, i64, i64) =
                (row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?);
            if id <= 0
                || !matches!(kind, 0 | 1)
                || size < 0
                || generation < 0
                || (kind == 1 && size != 0)
            {
                return Err(invalid("invalid object metadata"));
            }
            if kind == 0 {
                report.files += 1;
            } else {
                report.directories += 1;
            }
            report.logical_bytes = report
                .logical_bytes
                .checked_add(size as u64)
                .ok_or(Error::RecoveryLimit)?;
            objects.insert(
                id,
                Object {
                    kind,
                    size: size as u64,
                },
            );
        }
        if !objects.get(&1).is_some_and(|o| o.kind == 1) || report.logical_bytes > quota as u64 {
            return Err(invalid("invalid root or logical quota"));
        }
        let mut items = Vec::new();
        let mut links = BTreeMap::<i64, u64>::new();
        let mut keys = BTreeSet::new();
        let mut statement = self.db.prepare(
            "SELECT parent,object,display_name,name_key FROM entries ORDER BY parent,name_key",
        )?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let item = Item {
                parent: row.get(0)?,
                object: row.get(1)?,
                name: row.get(2)?,
            };
            let key: String = row.get(3)?;
            if names::key(&item.name).ok().as_ref() != Some(&key)
                || !keys.insert((item.parent, key))
                || item.object == 1
                || !objects.contains_key(&item.object)
                || !objects.get(&item.parent).is_some_and(|o| o.kind == 1)
            {
                return Err(invalid("invalid directory entry"));
            }
            *links.entry(item.object).or_default() += 1;
            items.push(item);
        }
        for (&id, object) in &objects {
            let count = links.get(&id).copied().unwrap_or(0);
            if object.kind == 1 && id != 1 && count != 1 {
                return Err(invalid("directory link count"));
            }
            if object.kind == 0 && count == 0 {
                report.orphan_files += 1;
            }
        }
        // Iterative traversal also rejects disconnected directory cycles.
        let mut children = BTreeMap::<i64, Vec<&Item>>::new();
        for item in &items {
            children.entry(item.parent).or_default().push(item);
        }
        let mut seen = BTreeSet::from([1]);
        let mut queue = VecDeque::from([(1, 0_usize)]);
        while let Some((parent, depth)) = queue.pop_front() {
            if let Some(entries) = children.get(&parent) {
                for item in entries {
                    if depth >= self.limits.depth {
                        return Err(Error::RecoveryLimit);
                    }
                    if objects[&item.object].kind == 1 {
                        if !seen.insert(item.object) {
                            return Err(invalid("directory cycle"));
                        }
                        queue.push_back((item.object, depth + 1));
                    }
                }
            }
        }
        if seen.len() as u64 != report.directories {
            return Err(invalid("unreachable directory"));
        }
        Ok(Catalog {
            objects,
            items,
            report,
        })
    }

    /// Strict verification, including unnamed orphan objects. Does not repair,
    /// unlink orphan data, guess missing bytes, or silently skip corrupt files.
    pub fn verify(&self) -> Result<RecoveryReport> {
        let mut catalog = self.catalog()?;
        let mut statement = self.db.prepare(
            "SELECT object,block,segment,offset,checksum FROM blocks ORDER BY object,block",
        )?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let object: i64 = row.get(0)?;
            let reference = segment::BlockRef {
                index: row.get(1)?,
                segment: row.get(2)?,
                offset: row.get(3)?,
                checksum: row.get(4)?,
            };
            let object = catalog
                .objects
                .get(&object)
                .ok_or_else(|| invalid("missing block object"))?;
            if object.kind != 0
                || reference.index < 0
                || reference.index as u64 >= object.size.div_ceil(BLOCK_SIZE as u64)
            {
                return Err(invalid("block outside file"));
            }
            segment::read(&self.evidence, &reference)?;
            catalog.report.blocks_checked += 1;
        }
        Ok(catalog.report)
    }

    /// New output directory containing files/, report.txt and completion marker.
    /// Any error leaves an INCOMPLETE tree and never overwrites an existing target.
    /// Only contents, names and selected link semantics are exported: no ACL/xattrs.
    pub fn export_new(&self, target: impl AsRef<Path>, mode: ExportLinks) -> Result<ExportReport> {
        self.verify()?;
        let catalog = self.catalog()?;
        let target = new_path(target.as_ref())?;
        if target.starts_with(&self.source) || target.starts_with(&self.workspace) {
            return Err(Error::InvalidRange);
        }
        let mut total = 0_u64;
        let mut counted = BTreeSet::new();
        for item in &catalog.items {
            let object = &catalog.objects[&item.object];
            if object.kind == 0 && (mode == ExportLinks::CopyAliases || counted.insert(item.object))
            {
                total = total.checked_add(object.size).ok_or(Error::RecoveryLimit)?;
                bounded(total, self.limits.export_bytes)?;
            }
        }
        fs::create_dir(&target)?;
        File::create_new(target.join("INCOMPLETE"))?.sync_all()?;
        let files = target.join("files");
        fs::create_dir(&files)?;
        let mut children = BTreeMap::<i64, Vec<&Item>>::new();
        for item in &catalog.items {
            children.entry(item.parent).or_default().push(item);
        }
        let mut paths = BTreeMap::<i64, PathBuf>::new();
        let mut queue = VecDeque::from([(1, files.clone())]);
        let mut report = ExportReport::default();
        while let Some((parent, directory)) = queue.pop_front() {
            if let Some(items) = children.get(&parent) {
                for item in items {
                    let path = directory.join(&item.name);
                    let object = &catalog.objects[&item.object];
                    if object.kind == 1 {
                        fs::create_dir(&path)?;
                        report.directories += 1;
                        queue.push_back((item.object, path));
                        continue;
                    }
                    report.files += 1;
                    if let Some(first) = paths.get(&item.object) {
                        if mode == ExportLinks::Preserve {
                            fs::hard_link(first, &path)?;
                            report.hard_links += 1;
                            continue;
                        }
                        report.copied_aliases += 1;
                    }
                    let mut output = File::create_new(&path)?;
                    // Export quota counts logical zeros, not just materialized blocks.
                    output.set_len(object.size)?;
                    let mut statement = self.db.prepare("SELECT block,segment,offset,checksum FROM blocks WHERE object=?1 ORDER BY block")?;
                    let mut rows = statement.query([item.object])?;
                    while let Some(row) = rows.next()? {
                        let reference = segment::BlockRef {
                            index: row.get(0)?,
                            segment: row.get(1)?,
                            offset: row.get(2)?,
                            checksum: row.get(3)?,
                        };
                        let data = segment::read(&self.evidence, &reference)?;
                        let offset = (reference.index as u64)
                            .checked_mul(BLOCK_SIZE as u64)
                            .ok_or(Error::InvalidRange)?;
                        let len = (object.size - offset).min(BLOCK_SIZE as u64) as usize;
                        output.seek(SeekFrom::Start(offset))?;
                        output.write_all(&data[..len])?;
                    }
                    output.sync_all()?;
                    report.bytes_written += object.size;
                    paths.entry(item.object).or_insert(path);
                }
            }
            segment::sync_directory(&directory)?;
        }
        let mut summary = File::create_new(target.join("report.txt"))?;
        writeln!(
            summary,
            "PortaWeave strict content export\n{report:#?}\nlink_mode={mode:?}\nunnamed_orphans_not_exported={}\nACL, timestamps, xattrs and ADS are NOT exported.\nNot a physical-persistence guarantee.",
            catalog.report.orphan_files
        )?;
        summary.sync_all()?;
        File::create_new(target.join("COMPLETE"))?.sync_all()?;
        fs::remove_file(target.join("INCOMPLETE"))?;
        segment::sync_directory(&target)?;
        Ok(report)
    }
}
