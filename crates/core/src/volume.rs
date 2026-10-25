use crate::{
    BLOCK_SIZE, CreateAttributes, Error, MAX_IO, MetadataUpdate, Result, SpacePolicy, SpaceStatus,
    Timestamp,
    catalog::CatalogVersion,
    metadata,
    owner::OwnerLock,
    segment::{self, BlockRef},
    segment_io::IoControl,
    space::SpaceGate,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct FileId(pub(crate) i64);

impl FileId {
    /// Stable catalog object ID. Only valid in its owning volume.
    pub const fn raw(self) -> i64 {
        self.0
    }

    /// Validate the wire representation, not object existence or permissions.
    pub fn from_raw(raw: i64) -> Result<Self> {
        if raw <= 0 {
            return Err(Error::InvalidRange);
        }
        Ok(Self(raw))
    }
}

pub const ROOT: FileId = FileId(1);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    File,
    Directory,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Handle(u64);
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct ReadView(u64);

/// Per-owner admission limits for copied snapshot indexes, NOT an exact
/// heap allocation or a byte limit on the underlying file contents.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadViewLimits {
    pub max_active_views: usize,
    pub max_indexed_block_refs: usize,
}
impl Default for ReadViewLimits {
    fn default() -> Self {
        Self {
            max_active_views: 128,
            max_indexed_block_refs: 65_536,
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Stat {
    pub id: FileId,
    pub kind: Kind,
    pub size: u64,
    pub links: u64,
    pub generation: u64,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    pub name: String,
    pub id: FileId,
}
/// A completed business mutation stays successful even if subsequent GC fails.
/// Failures latch write protection; adapters must surface this status separately.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MaintenanceStatus {
    Healthy,
    /// No data I/O attempted for compaction; retry on a later operation.
    DeferredSpace {
        available_bytes: u64,
        required_bytes: u64,
    },
    Failed {
        error: String,
    },
}
/// No pending garbage is counted as free, and the host can still race us.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapacitySnapshot {
    pub logical_quota_bytes: u64,
    pub logical_used_bytes: u64,
    pub host_available_bytes: u64,
    pub host_data_admission_bytes: u64,
    pub apparent_free_bytes: u64,
}

/// SQLite WAL checkpoint result; not a mounted filesystem fsync.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CheckpointStatus {
    pub wal_frames: u64,
    pub checkpointed_frames: u64,
}

/// Backend host-file barrier, not kernel-cache drain or safe device eject.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StorageSyncStatus {
    pub segment_files: u64,
    pub checkpoint: CheckpointStatus,
}

#[derive(Clone, Debug)]
pub struct Stats {
    pub maintenance: MaintenanceStatus,
    pub space: SpaceStatus,
    pub logical_bytes: u64,
    /// Data segment file lengths only; NOT total container allocation on exFAT.
    pub tracked_segment_bytes: u64,
    pub segments: usize,
    pub open_handles: usize,
    pub read_views: usize,
    pub indexed_view_block_refs: usize,
    pub read_view_limits: ReadViewLimits,
    pub portable_directory_sync_available: bool,
}
#[cfg(feature = "fault-injection")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum FaultPoint {
    AfterDataSync = 0,
    BeforeCommit = 1,
    AfterCommit = 2,
    BeforeCompactionCommit = 3,
    StorageSegmentsSynced = 4,
    StorageCatalogSynced = 5,
}

struct ViewState {
    size: u64,
    blocks: Vec<BlockRef>,
    range: Option<std::ops::Range<u64>>,
}
static TOKENS: AtomicU64 = AtomicU64::new(1);
fn token() -> Result<u64> {
    let mut current = TOKENS.load(Ordering::Relaxed);
    loop {
        let next = current.checked_add(1).ok_or(Error::InvalidRange)?;
        match TOKENS.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(value) => return Ok(value),
            Err(observed) => current = observed,
        }
    }
}

/// Experimental, serialized storage engine. Requires an exclusively controlled
/// backing directory. Not an OS filesystem or a security boundary against a
/// process which can modify the raw backing files. Never store the only copy.
pub struct Volume {
    // Drop the connection before the owner lock.
    pub(crate) db: Connection,
    pub(crate) catalog_version: CatalogVersion,
    root: PathBuf,
    handles: BTreeMap<Handle, FileId>,
    views: BTreeMap<ReadView, ViewState>,
    indexed_view_block_refs: usize,
    read_view_limits: ReadViewLimits,
    protected: Cell<bool>,
    maintenance: MaintenanceStatus,
    space: SpaceGate,
    io_control: IoControl,
    #[cfg(feature = "fault-injection")]
    fault: Option<FaultPoint>,
    #[cfg(feature = "fault-injection")]
    exit_on_fault: bool,
    _owner: OwnerLock,
}

impl Volume {
    /// Creates a NEW experimental container. Never opens or replaces an existing
    /// directory. An interrupted initialization is preserved for inspection.
    pub fn create(path: impl AsRef<Path>, quota: u64) -> Result<Self> {
        Self::create_internal(path.as_ref(), quota, None)
    }

    /// Explicit v2 creation. Does NOT migrate or open an existing v1 container.
    /// Root ownership/mode must come from the authorized caller, not this host.
    pub fn create_with_metadata(
        path: impl AsRef<Path>,
        quota: u64,
        root_attributes: CreateAttributes,
    ) -> Result<Self> {
        root_attributes.validate()?;
        Self::create_internal(path.as_ref(), quota, Some(root_attributes))
    }

    fn create_internal(
        path: &Path,
        quota: u64,
        root_attributes: Option<CreateAttributes>,
    ) -> Result<Self> {
        if quota > i64::MAX as u64 {
            return Err(Error::InvalidRange);
        }
        Self::sqlite_version_gate()?;
        let version = if root_attributes.is_some() {
            CatalogVersion::MetadataV2
        } else {
            CatalogVersion::BlocksV1
        };
        fs::create_dir(path)?;
        let root = fs::canonicalize(path)?;
        let owner = File::create_new(root.join("session.lock"))?;
        let owner = OwnerLock::acquire(owner)?;
        fs::create_dir(root.join("segments"))?;
        drop(File::create_new(root.join("catalog.sqlite"))?);
        let mut db = Connection::open_with_flags(
            root.join("catalog.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_WRITE,
        )?;
        db.execute_batch("PRAGMA auto_vacuum=INCREMENTAL;")?;
        Self::configure(&db)?;
        let tx = db.transaction()?;
        tx.execute_batch(version.schema())?;
        tx.execute(
            "INSERT INTO config VALUES (1, ?1, ?2)",
            params![quota as i64, version.marker()],
        )?;
        if let Some(attributes) = root_attributes {
            metadata::initialize(&tx, attributes)?;
        }
        tx.commit()?;
        let mut marker = File::create_new(root.join("format.txt"))?;
        marker.write_all(version.marker().as_bytes())?;
        marker.sync_all()?;
        segment::sync_directory(&root)?;
        Ok(Self::assemble(root, db, owner, version))
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::sqlite_version_gate()?;
        if !fs::symlink_metadata(path.as_ref())?.file_type().is_dir() {
            return Err(Error::UnsupportedFormat);
        }
        let root = fs::canonicalize(path)?;
        for name in ["format.txt", "catalog.sqlite", "session.lock"] {
            segment::regular(&root.join(name))?;
        }
        if fs::metadata(root.join("format.txt"))?.len() > 128
            || !fs::symlink_metadata(root.join("segments"))?
                .file_type()
                .is_dir()
        {
            return Err(Error::UnsupportedFormat);
        }
        let version = CatalogVersion::from_marker(&fs::read(root.join("format.txt"))?)?;
        for name in ["catalog.sqlite-wal", "catalog.sqlite-shm"] {
            match fs::symlink_metadata(root.join(name)) {
                Ok(m) if !m.file_type().is_file() => {
                    return Err(Error::Corrupt("nonregular SQLite auxiliary file".into()));
                }
                Ok(_) => (),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(e.into()),
            }
        }
        let owner = File::options()
            .read(true)
            .write(true)
            .open(root.join("session.lock"))?;
        let owner = OwnerLock::acquire(owner)?;
        let db = Connection::open_with_flags(
            root.join("catalog.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_WRITE,
        )?;
        db.execute_batch("PRAGMA trusted_schema=OFF;")?;
        if crate::catalog::validate_schema(&db)? != version {
            return Err(Error::UnsupportedFormat);
        }
        let format: String =
            db.query_row("SELECT format FROM config WHERE id=1", [], |r| r.get(0))?;
        if format != version.marker() {
            return Err(Error::UnsupportedFormat);
        }
        Self::configure(&db)?;
        let mut volume = Self::assemble(root, db, owner, version);
        // Validate all CURRENT references before any destructive reclamation.
        volume.verify()?;
        volume.mutate(|_| Ok(()))?;
        Ok(volume)
    }

    fn sqlite_version_gate() -> Result<()> {
        if rusqlite::version_number() < 3_051_003 {
            return Err(Error::UnsupportedFormat);
        }
        Ok(())
    }
    fn configure(db: &Connection) -> Result<()> {
        db.busy_timeout(std::time::Duration::ZERO)?;
        db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA fullfsync=ON; PRAGMA checkpoint_fullfsync=ON;")?;
        let mode: String = db.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
        if mode != "wal" {
            return Err(Error::UnsupportedFormat);
        }
        Ok(())
    }
    fn assemble(
        root: PathBuf,
        db: Connection,
        owner: OwnerLock,
        catalog_version: CatalogVersion,
    ) -> Self {
        Self {
            db,
            catalog_version,
            root,
            handles: BTreeMap::new(),
            views: BTreeMap::new(),
            indexed_view_block_refs: 0,
            read_view_limits: ReadViewLimits::default(),
            protected: Cell::new(false),
            maintenance: MaintenanceStatus::Healthy,
            space: SpaceGate::default(),
            io_control: IoControl::default(),
            #[cfg(feature = "fault-injection")]
            fault: None,
            #[cfg(feature = "fault-injection")]
            exit_on_fault: false,
            _owner: owner,
        }
    }
    /// A soft per-owner policy, not persistent allocation or an OS reservation.
    pub fn set_space_policy(&mut self, policy: SpacePolicy) -> Result<()> {
        policy.headroom()?;
        if self.protected.get() {
            return Err(Error::WriteProtected);
        }
        self.space.policy = policy;
        Ok(())
    }
    pub fn space_status(&self) -> Result<SpaceStatus> {
        self.observe(self.space.status(&self.root))
    }
    /// Test-only ceiling; never invents more capacity than the real host has.
    #[cfg(feature = "fault-injection")]
    pub fn set_test_available_space(&mut self, ceiling: Option<u64>) {
        self.space.available_ceiling = ceiling;
    }
    #[cfg(feature = "fault-injection")]
    pub fn inject_storage_fault_once(&mut self, fault: crate::StorageFault) -> Result<()> {
        if self.protected.get() {
            return Err(Error::WriteProtected);
        }
        if matches!(fault, crate::StorageFault::ShortWrites { max_bytes: 0 }) {
            return Err(Error::InvalidRange);
        }
        self.io_control.next_fault = Some(fault);
        Ok(())
    }
    /// Adjusts future read-view admission; cannot evict a live snapshot.
    /// A refused change never latches data write protection.
    pub fn set_read_view_limits(&mut self, limits: ReadViewLimits) -> Result<()> {
        if self.views.len() > limits.max_active_views
            || self.indexed_view_block_refs > limits.max_indexed_block_refs
        {
            return Err(Error::ResourceLimit);
        }
        self.read_view_limits = limits;
        Ok(())
    }
    /// A conservative capacity snapshot, not allocated/reserved disk space.
    pub fn capacity(&self) -> Result<CapacitySnapshot> {
        self.observe((|| {
            let quota: i64 = self
                .db
                .query_row("SELECT quota FROM config WHERE id=1", [], |r| r.get(0))?;
            let used: i64 =
                self.db
                    .query_row("SELECT COALESCE(SUM(size),0) FROM objects", [], |r| {
                        r.get(0)
                    })?;
            if quota < 0 || used < 0 || used > quota {
                return Err(Error::Corrupt("invalid quota or used bytes".into()));
            }
            let space = self.space.status(&self.root)?;
            Ok(CapacitySnapshot {
                logical_quota_bytes: quota as u64,
                logical_used_bytes: used as u64,
                host_available_bytes: space.host_available_bytes,
                host_data_admission_bytes: space.data_admission_bytes,
                apparent_free_bytes: ((quota - used) as u64).min(space.data_admission_bytes),
            })
        })())
    }

    /// FULL checkpoint of catalog.sqlite WAL only. Does NOT implement FSKit /
    /// WinFsp Flush: directory creation and OS/USB caches require separate
    /// platform persistence guarantees. Never translate this to fsync success.
    pub fn checkpoint_catalog(&mut self) -> Result<CheckpointStatus> {
        if self.protected.get() {
            return Err(Error::WriteProtected);
        }
        let result = (|| {
            let (busy, log, finished): (i64, i64, i64) =
                self.db.query_row("PRAGMA wal_checkpoint(FULL)", [], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?;
            if busy != 0 {
                return Err(Error::Busy);
            }
            if log < 0 || finished < 0 || finished != log {
                return Err(Error::Corrupt("incomplete SQLite FULL checkpoint".into()));
            }
            Ok(CheckpointStatus {
                wal_frames: log as u64,
                checkpointed_frames: finished as u64,
            })
        })();
        self.observe(result)
    }

    /// Synchronize tracked segment files, their directory, the SQLite catalog
    /// and WAL, the format marker and container directory in this serial owner.
    /// A native adapter must first drain its own/OS pending writes. This is not
    /// safe eject, hardware certification or a Windows directory-flush fallback.
    pub fn synchronize_storage(&mut self) -> Result<StorageSyncStatus> {
        #[cfg(not(unix))]
        {
            Err(Error::UnsupportedOperation)
        }
        #[cfg(unix)]
        {
            if self.protected.get() {
                return Err(Error::WriteProtected);
            }
            let result = (|| {
                let mut segment_files = 0_u64;
                {
                    let mut statement = self.db.prepare("SELECT id FROM segments ORDER BY id")?;
                    let mut rows = statement.query([])?;
                    while let Some(row) = rows.next()? {
                        let id: i64 = row.get(0)?;
                        if id <= 0 {
                            return Err(Error::Corrupt("invalid segment identity".into()));
                        }
                        let path = segment::path(&self.root, id);
                        segment::regular(&path)?;
                        File::open(path)?.sync_all()?;
                        segment_files = segment_files.checked_add(1).ok_or(Error::ResourceLimit)?;
                    }
                }
                segment::sync_directory(&self.root.join("segments"))?;
                self.hit(4)?;
                let checkpoint = self.checkpoint_catalog()?;
                for name in ["catalog.sqlite", "format.txt"] {
                    let path = self.root.join(name);
                    segment::regular(&path)?;
                    File::open(path)?.sync_all()?;
                }
                let wal = self.root.join("catalog.sqlite-wal");
                match fs::symlink_metadata(&wal) {
                    Ok(meta) if meta.file_type().is_file() => File::open(&wal)?.sync_all()?,
                    Ok(_) => return Err(Error::Corrupt("nonregular SQLite WAL".into())),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                    Err(error) => return Err(error.into()),
                }
                self.hit(5)?;
                segment::sync_directory(&self.root)?;
                Ok(StorageSyncStatus {
                    segment_files,
                    checkpoint,
                })
            })();
            self.observe(result)
        }
    }

    pub fn is_write_protected(&self) -> bool {
        self.protected.get()
    }
    pub(crate) fn observe<T>(&self, result: Result<T>) -> Result<T> {
        if matches!(
            &result,
            Err(Error::Io(_) | Error::Sql(_) | Error::Corrupt(_) | Error::Injected)
        ) {
            self.protected.set(true);
        }
        result
    }
    pub fn maintenance_status(&self) -> &MaintenanceStatus {
        &self.maintenance
    }
    // Only business errors are returned as operation errors. A failing commit
    // itself can still be indeterminate; Err never promises universal rollback.
    pub(crate) fn mutate<T>(&mut self, action: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        if self.protected.get() {
            return Err(Error::WriteProtected);
        }
        let result = action(self);
        let value = self.observe(result)?;
        match self.hit(2).and_then(|_| self.maintain()) {
            Ok(()) => self.maintenance = MaintenanceStatus::Healthy,
            Err(Error::HostSpace {
                available_bytes,
                required_bytes,
            }) => {
                self.maintenance = MaintenanceStatus::DeferredSpace {
                    available_bytes,
                    required_bytes,
                };
            }
            Err(error) => {
                self.protected.set(true);
                self.maintenance = MaintenanceStatus::Failed {
                    error: error.to_string(),
                };
            }
        }
        Ok(value)
    }
    #[cfg(feature = "fault-injection")]
    pub fn inject_once(&mut self, point: FaultPoint) {
        self.fault = Some(point);
        self.exit_on_fault = false;
    }
    /// Test-only process termination, including INSIDE an active SQLite transaction.
    /// Never enable the fault-injection feature for a user-facing application.
    #[cfg(feature = "fault-injection")]
    pub fn inject_process_exit_once(&mut self, point: FaultPoint) {
        self.fault = Some(point);
        self.exit_on_fault = true;
    }
    pub(crate) fn take_fault(&mut self, point: u8) -> Option<bool> {
        #[cfg(feature = "fault-injection")]
        if self.fault.is_some_and(|f| f as u8 == point) {
            self.fault = None;
            return Some(self.exit_on_fault);
        }
        let _ = point;
        None
    }
    pub(crate) fn interrupt(action: Option<bool>) -> Result<()> {
        match action {
            Some(true) => std::process::exit(86),
            Some(false) => Err(Error::Injected),
            None => Ok(()),
        }
    }
    fn hit(&mut self, point: u8) -> Result<()> {
        Self::interrupt(self.take_fault(point))
    }

    pub fn stat(&self, id: FileId) -> Result<Stat> {
        self.observe((|| {
            let row: Option<(i64, i64, i64, i64)> = self.db.query_row(
                "SELECT kind,size,generation,(SELECT COUNT(*) FROM entries WHERE object=objects.id) FROM objects WHERE id=?1", [id.0],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).optional()?;
            let (kind, size, generation, links) = row.ok_or(Error::NotFound)?;
            if size < 0 || generation < 0 || links < 0 { return Err(Error::Corrupt("invalid object metadata".into())); }
            let kind = match kind { 0 => Kind::File, 1 => Kind::Directory, _ => return Err(Error::Corrupt("invalid kind".into())) };
            Ok(Stat { id, kind, size: size as u64, links: links as u64, generation: generation as u64 })
        })())
    }
    pub(crate) fn directory(&self, id: FileId) -> Result<()> {
        if self.stat(id)?.kind != Kind::Directory {
            return Err(Error::NotDirectory);
        }
        Ok(())
    }
    pub fn open_file(&mut self, id: FileId) -> Result<Handle> {
        if self.stat(id)?.kind != Kind::File {
            return Err(Error::IsDirectory);
        }
        let handle = Handle(token()?);
        self.handles.insert(handle, id);
        Ok(handle)
    }
    pub(crate) fn object(&self, handle: Handle) -> Result<FileId> {
        self.handles
            .get(&handle)
            .copied()
            .ok_or(Error::InvalidHandle)
    }
    pub(crate) fn open_handle_count(&self) -> usize {
        self.handles.len()
    }
    pub fn close_file(&mut self, handle: Handle) -> Result<()> {
        // Closing remains possible in protection mode, but does not run GC there.
        self.handles.remove(&handle).ok_or(Error::InvalidHandle)?;
        if self.protected.get() {
            return Ok(());
        }
        self.mutate(|_| Ok(()))
    }
    fn refs(&self, id: FileId) -> Result<Vec<BlockRef>> {
        let mut statement = self.db.prepare(
            "SELECT block,segment,offset,checksum FROM blocks WHERE object=?1 ORDER BY block",
        )?;
        let rows = statement.query_map([id.0], |r| {
            Ok(BlockRef {
                index: r.get(0)?,
                segment: r.get(1)?,
                offset: r.get(2)?,
                checksum: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }
    fn refs_range(&self, id: FileId, first: i64, last: i64) -> Result<Vec<BlockRef>> {
        let mut statement = self.db.prepare(
            "SELECT block,segment,offset,checksum FROM blocks WHERE object=?1 AND block BETWEEN ?2 AND ?3 ORDER BY block",
        )?;
        let rows = statement.query_map(params![id.0, first, last], |r| {
            Ok(BlockRef {
                index: r.get(0)?,
                segment: r.get(1)?,
                offset: r.get(2)?,
                checksum: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }
    pub fn acquire_view(&mut self, handle: Handle) -> Result<ReadView> {
        self.acquire_indexed_view(handle, None)
    }
    /// Retain only the block mappings intersecting this byte interval. Reads
    /// use absolute file offsets and must fit entirely inside the interval.
    /// The original EOF is retained, including for sparse/beyond-EOF leases.
    pub fn acquire_range_view(
        &mut self,
        handle: Handle,
        offset: u64,
        length: u64,
    ) -> Result<ReadView> {
        let end = offset
            .checked_add(length)
            .filter(|end| *end <= i64::MAX as u64)
            .ok_or(Error::InvalidRange)?;
        self.acquire_indexed_view(handle, Some(offset..end))
    }
    fn acquire_indexed_view(
        &mut self,
        handle: Handle,
        range: Option<std::ops::Range<u64>>,
    ) -> Result<ReadView> {
        let id = self.object(handle)?;
        if self.views.len() >= self.read_view_limits.max_active_views {
            return Err(Error::ResourceLimit);
        }
        // Probe the SQLite index size BEFORE allocating its Vec. An active
        // view retains its exact old block mapping across rename/overwrite/GC.
        let (first, last) = match &range {
            Some(r) if r.is_empty() => (1, 0),
            Some(r) => (
                (r.start / BLOCK_SIZE as u64) as i64,
                ((r.end - 1) / BLOCK_SIZE as u64) as i64,
            ),
            None => (0, i64::MAX),
        };
        let count: i64 = self.observe(
            self.db
                .query_row(
                    "SELECT COUNT(*) FROM blocks WHERE object=?1 AND block BETWEEN ?2 AND ?3",
                    params![id.0, first, last],
                    |r| r.get(0),
                )
                .map_err(Error::from),
        )?;
        let count = usize::try_from(count)
            .map_err(|_| Error::Corrupt("negative or oversized block-ref count".into()))?;
        let next = self
            .indexed_view_block_refs
            .checked_add(count)
            .ok_or(Error::ResourceLimit)?;
        if next > self.read_view_limits.max_indexed_block_refs {
            return Err(Error::ResourceLimit);
        }
        let size = self.stat(id)?.size;
        let blocks = self.observe(self.refs_range(id, first, last))?;
        if blocks.len() != count {
            self.protected.set(true);
            return Err(Error::Corrupt(
                "read-view index changed during acquisition".into(),
            ));
        }
        let view = ReadView(token()?);
        self.views.insert(
            view,
            ViewState {
                size,
                blocks,
                range,
            },
        );
        self.indexed_view_block_refs = next;
        Ok(view)
    }
    pub fn release_view(&mut self, view: ReadView) -> Result<()> {
        let old = self.views.remove(&view).ok_or(Error::InvalidHandle)?;
        self.indexed_view_block_refs -= old.blocks.len();
        if self.protected.get() {
            return Ok(());
        }
        self.mutate(|_| Ok(()))
    }
    pub fn read(&self, handle: Handle, offset: u64, len: usize) -> Result<Vec<u8>> {
        self.observe((|| {
            let id = self.object(handle)?;
            let size = self.stat(id)?.size;
            if len > MAX_IO || offset > i64::MAX as u64 {
                return Err(Error::InvalidRange);
            }
            let available = size.saturating_sub(offset).min(len as u64);
            if available == 0 {
                return Ok(Vec::new());
            }
            let first = (offset / BLOCK_SIZE as u64) as i64;
            let last = ((offset + available - 1) / BLOCK_SIZE as u64) as i64;
            let refs = self.refs_range(id, first, last)?;
            self.read_refs(size, &refs, offset, len)
        })())
    }
    /// A bounded in-flight snapshot lease, not an OS memory mapping or a user snapshot.
    pub fn read_view(&self, view: ReadView, offset: u64, len: usize) -> Result<Vec<u8>> {
        self.observe((|| {
            let state = self.views.get(&view).ok_or(Error::InvalidHandle)?;
            if let Some(range) = &state.range {
                let end = offset.checked_add(len as u64).ok_or(Error::InvalidRange)?;
                if offset < range.start || end > range.end {
                    return Err(Error::InvalidRange);
                }
            }
            self.read_refs(state.size, &state.blocks, offset, len)
        })())
    }
    fn read_refs(&self, size: u64, refs: &[BlockRef], offset: u64, len: usize) -> Result<Vec<u8>> {
        if len > MAX_IO || offset > i64::MAX as u64 {
            return Err(Error::InvalidRange);
        }
        let len = (size.saturating_sub(offset)).min(len as u64) as usize;
        let mut out = vec![0; len];
        if len == 0 {
            return Ok(out);
        }
        let end = offset + len as u64;
        for reference in refs {
            let start = u64::try_from(reference.index)
                .map_err(|_| Error::Corrupt("negative block".into()))?
                .checked_mul(BLOCK_SIZE as u64)
                .ok_or_else(|| Error::Corrupt("block overflow".into()))?;
            let stop = start
                .checked_add(BLOCK_SIZE as u64)
                .ok_or_else(|| Error::Corrupt("block overflow".into()))?;
            if stop <= offset || start >= end {
                continue;
            }
            let bytes = segment::read(&self.root, reference)?;
            let lo = start.max(offset);
            let hi = stop.min(end);
            out[(lo - offset) as usize..(hi - offset) as usize]
                .copy_from_slice(&bytes[(lo - start) as usize..(hi - start) as usize]);
        }
        Ok(out)
    }
    fn quota(&self, old: u64, new: u64) -> Result<()> {
        let used: i64 =
            self.db
                .query_row("SELECT COALESCE(SUM(size),0) FROM objects", [], |r| {
                    r.get(0)
                })?;
        let quota: i64 = self
            .db
            .query_row("SELECT quota FROM config WHERE id=1", [], |r| r.get(0))?;
        let total = (used as u64)
            .checked_sub(old)
            .and_then(|n| n.checked_add(new))
            .ok_or(Error::InvalidRange)?;
        if new > i64::MAX as u64 || total > quota as u64 {
            return Err(Error::Quota);
        }
        Ok(())
    }
    fn stage(&mut self, blocks: &[(i64, Vec<u8>)]) -> Result<Vec<BlockRef>> {
        let bytes = segment::encoded_len(blocks.len())?;
        // Checked before even allocating a catalog identity. This is only a
        // conservative admission snapshot; real I/O errors must still propagate.
        self.space.admit(&self.root, bytes)?;
        // Reserve a never-reused identity before I/O. An interrupted stage stays
        // unreferenced; recovery can remove it without touching committed data.
        self.db.execute("INSERT INTO segments DEFAULT VALUES", [])?;
        let id = self.db.last_insert_rowid();
        let refs = segment::write(&self.root, id, blocks, &mut self.io_control)?;
        self.db.execute(
            "UPDATE segments SET bytes=?1 WHERE id=?2",
            params![i64::try_from(bytes).map_err(|_| Error::InvalidRange)?, id],
        )?;
        Ok(refs)
    }
    fn bind(tx: &rusqlite::Transaction<'_>, id: FileId, refs: &[BlockRef]) -> Result<()> {
        for r in refs {
            tx.execute("INSERT INTO blocks(object,block,segment,offset,checksum) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(object,block) DO UPDATE SET segment=excluded.segment,offset=excluded.offset,checksum=excluded.checksum",
                params![id.0, r.index, r.segment, r.offset, &r.checksum])?;
        }
        Ok(())
    }
    pub fn write(&mut self, handle: Handle, offset: u64, data: &[u8]) -> Result<usize> {
        self.mutate(|this| {
            let id = this.object(handle)?;
            if data.len() > MAX_IO || offset > i64::MAX as u64 {
                return Err(Error::InvalidRange);
            }
            if data.is_empty() {
                return Ok(0);
            }
            let end = offset
                .checked_add(data.len() as u64)
                .filter(|e| *e <= i64::MAX as u64)
                .ok_or(Error::InvalidRange)?;
            let old = this.stat(id)?.size;
            let new = old.max(end);
            this.quota(old, new)?;
            let first = offset / BLOCK_SIZE as u64;
            let last = (end - 1) / BLOCK_SIZE as u64;
            let old_refs = this.refs_range(id, first as i64, last as i64)?;
            let mut changed = Vec::new();
            for index in first..=last {
                let mut block = match old_refs.iter().find(|r| r.index == index as i64) {
                    Some(r) => segment::read(&this.root, r)?,
                    None => vec![0; BLOCK_SIZE],
                };
                let start = index * BLOCK_SIZE as u64;
                let lo = start.max(offset);
                let hi = (start + BLOCK_SIZE as u64).min(end);
                block[(lo - start) as usize..(hi - start) as usize]
                    .copy_from_slice(&data[(lo - offset) as usize..(hi - offset) as usize]);
                changed.push((index as i64, block));
            }
            let refs = this.stage(&changed)?;
            this.hit(0)?;
            let time = Timestamp::now()?;
            let before = this.take_fault(1);
            let tx = this.db.transaction()?;
            Self::bind(&tx, id, &refs)?;
            tx.execute(
                "UPDATE objects SET size=?1,generation=generation+1 WHERE id=?2",
                params![new as i64, id.0],
            )?;
            metadata::touch(&tx, this.catalog_version, id, time, true, true)?;
            Self::interrupt(before)?;
            tx.commit()?;
            Ok(data.len())
        })
    }
    pub fn truncate(&mut self, handle: Handle, size: u64) -> Result<()> {
        self.truncate_internal(handle, size, MetadataUpdate::default())
    }
    /// Trusted v2 primitive: size/blocks and requested metadata commit together.
    /// Authorization remains the native adapter's responsibility.
    pub fn truncate_with_metadata(
        &mut self,
        handle: Handle,
        size: u64,
        update: MetadataUpdate,
    ) -> Result<()> {
        self.require_metadata()?;
        metadata::validate_update(update)?;
        self.truncate_internal(handle, size, update)
    }
    fn truncate_internal(
        &mut self,
        handle: Handle,
        size: u64,
        update: MetadataUpdate,
    ) -> Result<()> {
        self.mutate(|this| {
            let id = this.object(handle)?;
            let old = this.stat(id)?.size;
            this.quota(old, size)?;
            let mut tail = Vec::new();
            if size < old && !size.is_multiple_of(BLOCK_SIZE as u64) {
                let index = (size / BLOCK_SIZE as u64) as i64;
                if let Some(r) = this.refs_range(id, index, index)?.into_iter().next() {
                    let mut bytes = segment::read(&this.root, &r)?;
                    bytes[(size % BLOCK_SIZE as u64) as usize..].fill(0);
                    tail = this.stage(&[(index, bytes)])?;
                    this.hit(0)?;
                }
            }
            let before = this.take_fault(1);
            let time = Timestamp::now()?;
            let tx = this.db.transaction()?;
            tx.execute(
                "DELETE FROM blocks WHERE object=?1 AND block>=?2",
                params![id.0, size.div_ceil(BLOCK_SIZE as u64) as i64],
            )?;
            Self::bind(&tx, id, &tail)?;
            tx.execute(
                "UPDATE objects SET size=?1,generation=generation+1 WHERE id=?2",
                params![size as i64, id.0],
            )?;
            metadata::apply_update(
                &tx,
                this.catalog_version,
                id,
                Kind::File,
                update,
                time,
                true,
            )?;
            Self::interrupt(before)?;
            tx.commit()?;
            Ok(())
        })
    }

    fn pinned_segments(&self) -> BTreeSet<i64> {
        self.views
            .values()
            .flat_map(|v| v.blocks.iter().map(|b| b.segment))
            .collect()
    }
    fn dead_segments(&mut self) -> Result<()> {
        let pinned = self.pinned_segments();
        let dead: Vec<i64> = self.db.prepare("SELECT id FROM segments WHERE NOT EXISTS (SELECT 1 FROM blocks WHERE segment=segments.id)")?
            .query_map([], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?;
        for id in dead {
            if pinned.contains(&id) {
                continue;
            }
            // Names are generated exclusively from positive database identities.
            if id <= 0 {
                return Err(Error::Corrupt("invalid segment identity".into()));
            }
            match fs::remove_file(segment::path(&self.root, id)) {
                Ok(()) => segment::sync_directory(&self.root.join("segments"))?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(e.into()),
            }
            self.db.execute("DELETE FROM segments WHERE id=?1", [id])?;
        }
        Ok(())
    }
    fn maintain(&mut self) -> Result<()> {
        let orphan: Vec<i64> = self.db.prepare("SELECT id FROM objects WHERE id<>1 AND kind=0 AND NOT EXISTS (SELECT 1 FROM entries WHERE object=objects.id)")?
            .query_map([], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?;
        let tx = self.db.transaction()?;
        for id in orphan {
            if !self.handles.values().any(|p| p.0 == id) {
                tx.execute("DELETE FROM objects WHERE id=?1", [id])?;
            }
        }
        tx.commit()?;
        self.dead_segments()?;
        self.compact_one()?;
        self.dead_segments()
    }
    /// One serialized compaction per mutation. Idle scheduling and I/O budgets
    /// remain pending; no worker thread is implied by this prototype.
    fn compact_one(&mut self) -> Result<()> {
        let pinned = self.pinned_segments();
        let candidates: Vec<i64> = self.db.prepare("SELECT s.id FROM segments s JOIN blocks b ON b.segment=s.id GROUP BY s.id HAVING COUNT(*)*65536 <= (s.bytes-24)/2 ORDER BY s.id")?
            .query_map([], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?;
        let Some(id) = candidates.into_iter().find(|s| !pinned.contains(s)) else {
            return Ok(());
        };
        let records: Vec<(i64, BlockRef)> = self.db.prepare("SELECT object,block,offset,checksum FROM blocks WHERE segment=?1 ORDER BY object,block")?
            .query_map([id], |r| Ok((r.get(0)?, BlockRef { index: r.get(1)?, segment: id, offset: r.get(2)?, checksum: r.get(3)? })))?
            .collect::<std::result::Result<_, _>>()?;
        let mut payload = Vec::with_capacity(records.len());
        for (_, old) in &records {
            payload.push((old.index, segment::read(&self.root, old)?));
        }
        let refs = self.stage(&payload)?;
        let before = self.take_fault(3);
        let tx = self.db.transaction()?;
        for ((object, old), new) in records.iter().zip(&refs) {
            let changed = tx.execute("UPDATE blocks SET segment=?1,offset=?2,checksum=?3 WHERE object=?4 AND block=?5 AND segment=?6 AND offset=?7",
                params![new.segment, new.offset, &new.checksum, object, old.index, old.segment, old.offset])?;
            if changed != 1 {
                return Err(Error::Corrupt("compaction reference changed".into()));
            }
        }
        Self::interrupt(before)?;
        tx.commit()?;
        Ok(())
    }
    pub fn stats(&self) -> Result<Stats> {
        self.observe((|| {
            let logical_raw: i64 =
                self.db
                    .query_row("SELECT COALESCE(SUM(size),0) FROM objects", [], |r| {
                        r.get(0)
                    })?;
            let logical_bytes = u64::try_from(logical_raw)
                .map_err(|_| Error::Corrupt("negative logical usage".into()))?;
            let ids: Vec<i64> = self
                .db
                .prepare("SELECT id FROM segments")?
                .query_map([], |r| r.get(0))?
                .collect::<std::result::Result<_, _>>()?;
            let mut bytes = 0_u64;
            for id in &ids {
                match fs::metadata(segment::path(&self.root, *id)) {
                    Ok(m) => bytes = bytes.checked_add(m.len()).ok_or(Error::InvalidRange)?,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                    Err(e) => return Err(e.into()),
                }
            }
            Ok(Stats {
                maintenance: self.maintenance.clone(),
                space: self.space_status()?,
                logical_bytes,
                tracked_segment_bytes: bytes,
                segments: ids.len(),
                open_handles: self.handles.len(),
                read_views: self.views.len(),
                indexed_view_block_refs: self.indexed_view_block_refs,
                read_view_limits: self.read_view_limits,
                portable_directory_sync_available: cfg!(unix),
            })
        })())
    }
    /// Checks current references. Not a repair API, forensic read-only open, or
    /// proof of on-device persistence. Caller must own this container exclusively.
    pub fn verify(&self) -> Result<()> {
        self.observe((|| {
            let check: String = self.db.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
            if check != "ok" {
                return Err(Error::Corrupt(check));
            }
            if self.db.prepare("PRAGMA foreign_key_check")?.exists([])? {
                return Err(Error::Corrupt("foreign key violation".into()));
            }
            metadata::verify(&self.db, self.catalog_version)?;
            if self.stat(ROOT)?.kind != Kind::Directory || self.stat(ROOT)?.links != 0 {
                return Err(Error::Corrupt("invalid root".into()));
            }
            let bad: i64 = self.db.query_row(
                "SELECT COUNT(*) FROM entries e JOIN objects p ON p.id=e.parent WHERE p.kind<>1",
                [],
                |r| r.get(0),
            )?;
            if bad != 0 {
                return Err(Error::Corrupt("nondirectory parent".into()));
            }
            let names: Vec<(String, String)> = self
                .db
                .prepare("SELECT name_key,display_name FROM entries")?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<std::result::Result<_, _>>()?;
            for (key, display) in names {
                if crate::names::key(&display).ok().as_ref() != Some(&key) {
                    return Err(Error::Corrupt("invalid name comparison key".into()));
                }
            }
            let ids: Vec<i64> = self
                .db
                .prepare("SELECT id FROM objects")?
                .query_map([], |r| r.get(0))?
                .collect::<std::result::Result<_, _>>()?;
            for id in ids {
                if id <= 0 {
                    return Err(Error::Corrupt("invalid object identity".into()));
                }
                let stat = self.stat(FileId(id))?;
                if stat.kind == Kind::Directory {
                    if stat.size != 0 {
                        return Err(Error::Corrupt("directory size".into()));
                    }
                    let mut cursor = FileId(id);
                    let mut seen = BTreeSet::new();
                    while cursor != ROOT {
                        if !seen.insert(cursor) {
                            return Err(Error::Corrupt("directory cycle".into()));
                        }
                        if self.stat(cursor)?.links != 1 {
                            return Err(Error::Corrupt("directory link count".into()));
                        }
                        cursor = FileId(self.db.query_row(
                            "SELECT parent FROM entries WHERE object=?1",
                            [cursor.0],
                            |r| r.get(0),
                        )?);
                    }
                }
                for reference in self.refs(FileId(id))? {
                    if stat.kind != Kind::File
                        || reference.index < 0
                        || reference.index as u64 >= stat.size.div_ceil(BLOCK_SIZE as u64)
                    {
                        return Err(Error::Corrupt("block outside file".into()));
                    }
                    segment::read(&self.root, &reference)?;
                }
            }
            Ok(())
        })())
    }
}
