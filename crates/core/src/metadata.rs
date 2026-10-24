//! Opt-in v2 metadata, stored in the SAME SQLite transactions as content/names.
//! These trusted APIs do not authenticate a caller or implement OS permissions.
use crate::{Error, FileId, Kind, Result, Volume, catalog::CatalogVersion};
use rusqlite::{Connection, OptionalExtension, params};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Timestamp {
    /// Signed Unix seconds; nanoseconds are always a nonnegative fraction.
    pub seconds: i64,
    pub nanoseconds: u32,
}
impl Timestamp {
    pub fn now() -> Result<Self> {
        let (seconds, nanoseconds) = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => (
                i64::try_from(d.as_secs()).map_err(|_| Error::InvalidRange)?,
                d.subsec_nanos(),
            ),
            Err(e) => {
                let d = e.duration();
                let seconds = i64::try_from(d.as_secs()).map_err(|_| Error::InvalidRange)?;
                if d.subsec_nanos() == 0 {
                    (-seconds, 0)
                } else {
                    (
                        seconds
                            .checked_neg()
                            .and_then(|s| s.checked_sub(1))
                            .ok_or(Error::InvalidRange)?,
                        1_000_000_000 - d.subsec_nanos(),
                    )
                }
            }
        };
        Ok(Self {
            seconds,
            nanoseconds,
        })
    }
    fn validate(self) -> Result<()> {
        if self.nanoseconds >= 1_000_000_000 {
            return Err(Error::InvalidRange);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CreateAttributes {
    pub uid: u32,
    pub gid: u32,
    /// POSIX permission/special bits only, 0..=0o7777. Kind is stored separately.
    pub mode: u16,
}
impl CreateAttributes {
    pub(crate) fn validate(self) -> Result<()> {
        if self.mode > 0o7777 {
            return Err(Error::InvalidRange);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Metadata {
    pub uid: u32,
    pub gid: u32,
    pub mode: u16,
    pub created: Timestamp,
    pub accessed: Timestamp,
    pub modified: Timestamp,
    pub changed: Timestamp,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MetadataUpdate {
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub mode: Option<u16>,
    pub created: Option<Timestamp>,
    pub accessed: Option<Timestamp>,
    pub modified: Option<Timestamp>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VolumeIdentity {
    /// Persistent UUID v4 bytes, not a credential or cryptographic secret.
    pub uuid: [u8; 16],
    /// v2 currently defines only bit 0: POSIX metadata storage.
    pub capabilities: u64,
}

pub(crate) fn initialize(db: &Connection, attrs: CreateAttributes) -> Result<()> {
    // Existing bundled SQLite PRNG, no OS identity inference or new dependency.
    let bytes: Vec<u8> = db.query_row("SELECT randomblob(16)", [], |r| r.get(0))?;
    let mut uuid: [u8; 16] = bytes
        .try_into()
        .map_err(|_| Error::Corrupt("UUID length".into()))?;
    uuid[6] = (uuid[6] & 0x0f) | 0x40;
    uuid[8] = (uuid[8] & 0x3f) | 0x80;
    db.execute("INSERT INTO volume_identity VALUES (1,?1,1)", [&uuid[..]])?;
    insert(db, FileId(1), attrs, Timestamp::now()?)
}

pub(crate) fn identity(db: &Connection) -> Result<VolumeIdentity> {
    let (bytes, capabilities): (Vec<u8>, i64) = db
        .query_row(
            "SELECT uuid,capabilities FROM volume_identity WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| Error::Corrupt("missing volume identity".into()))?;
    let uuid: [u8; 16] = bytes
        .try_into()
        .map_err(|_| Error::Corrupt("UUID length".into()))?;
    if uuid[6] >> 4 != 4 || uuid[8] >> 6 != 2 || capabilities != 1 {
        return Err(Error::Corrupt(
            "unsupported volume identity or capabilities".into(),
        ));
    }
    Ok(VolumeIdentity {
        uuid,
        capabilities: capabilities as u64,
    })
}

pub(crate) fn get(db: &Connection, id: FileId) -> Result<Metadata> {
    db.query_row(
        "SELECT uid,gid,mode,birth_sec,birth_nsec,access_sec,access_nsec,modify_sec,modify_nsec,change_sec,change_nsec FROM object_metadata WHERE object=?1",
        [id.raw()], |r| Ok(Metadata {
            uid: r.get(0)?, gid: r.get(1)?, mode: r.get(2)?,
            created: Timestamp { seconds: r.get(3)?, nanoseconds: r.get(4)? },
            accessed: Timestamp { seconds: r.get(5)?, nanoseconds: r.get(6)? },
            modified: Timestamp { seconds: r.get(7)?, nanoseconds: r.get(8)? },
            changed: Timestamp { seconds: r.get(9)?, nanoseconds: r.get(10)? },
        }),
    ).optional()?.ok_or_else(|| Error::Corrupt("missing object metadata".into()))
}

pub(crate) fn insert(db: &Connection, id: FileId, a: CreateAttributes, t: Timestamp) -> Result<()> {
    a.validate()?;
    db.execute(
        "INSERT INTO object_metadata VALUES (?1,?2,?3,?4,?5,?6,?5,?6,?5,?6,?5,?6)",
        params![id.raw(), a.uid, a.gid, a.mode, t.seconds, t.nanoseconds],
    )?;
    Ok(())
}

/// `data` updates mtime; `clear_special` strips setuid/setgid after content writes.
/// Called before commit, never as post-commit best-effort maintenance.
pub(crate) fn touch(
    db: &Connection,
    version: CatalogVersion,
    id: FileId,
    t: Timestamp,
    data: bool,
    clear_special: bool,
) -> Result<()> {
    if version == CatalogVersion::BlocksV1 {
        return Ok(());
    }
    let changed = db.execute(
        "UPDATE object_metadata SET change_sec=?1,change_nsec=?2,modify_sec=CASE WHEN ?3 THEN ?1 ELSE modify_sec END,modify_nsec=CASE WHEN ?3 THEN ?2 ELSE modify_nsec END,mode=mode & ?4 WHERE object=?5",
        params![t.seconds, t.nanoseconds, data, if clear_special { 0o1777 } else { 0o7777 }, id.raw()],
    )?;
    if changed != 1 {
        return Err(Error::Corrupt(
            "missing object metadata during mutation".into(),
        ));
    }
    Ok(())
}

pub(crate) fn verify(db: &Connection, version: CatalogVersion) -> Result<()> {
    if version == CatalogVersion::BlocksV1 {
        return Ok(());
    }
    identity(db)?;
    if db.prepare("SELECT 1 FROM objects o LEFT JOIN object_metadata m ON m.object=o.id WHERE m.object IS NULL LIMIT 1")?.exists([])? {
        return Err(Error::Corrupt("incomplete object metadata".into()));
    }
    // Streaming validation: do not allocate a second object-sized map.
    let mut statement = db.prepare("SELECT object FROM object_metadata")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let m = get(db, FileId(row.get(0)?))?;
        CreateAttributes {
            uid: m.uid,
            gid: m.gid,
            mode: m.mode,
        }
        .validate()
        .map_err(|_| Error::Corrupt("invalid metadata mode".into()))?;
        for t in [m.created, m.accessed, m.modified, m.changed] {
            t.validate()
                .map_err(|_| Error::Corrupt("invalid metadata timestamp".into()))?;
        }
    }
    Ok(())
}

pub(crate) fn validate_update(update: MetadataUpdate) -> Result<()> {
    if update.mode.is_some_and(|mode| mode > 0o7777) {
        return Err(Error::InvalidRange);
    }
    for time in [update.created, update.accessed, update.modified]
        .into_iter()
        .flatten()
    {
        time.validate()?;
    }
    Ok(())
}

/// Caller owns the surrounding business transaction. Explicit mtime wins over
/// the truncate clock; content/ownership changes always clear file setid bits,
/// including when a combined request supplied them in its requested mode.
pub(crate) fn apply_update(
    db: &Connection,
    version: CatalogVersion,
    id: FileId,
    kind: Kind,
    update: MetadataUpdate,
    time: Timestamp,
    content_changed: bool,
) -> Result<()> {
    if version == CatalogVersion::BlocksV1 {
        return Ok(());
    }
    let old = get(db, id)?;
    let uid = update.uid.unwrap_or(old.uid);
    let gid = update.gid.unwrap_or(old.gid);
    let mut mode = update.mode.unwrap_or(old.mode);
    if kind == Kind::File && (content_changed || uid != old.uid || gid != old.gid) {
        mode &= 0o1777;
    }
    let b = update.created.unwrap_or(old.created);
    let a = update.accessed.unwrap_or(old.accessed);
    let m = update
        .modified
        .unwrap_or(if content_changed { time } else { old.modified });
    let changed = db.execute("UPDATE object_metadata SET uid=?1,gid=?2,mode=?3,birth_sec=?4,birth_nsec=?5,access_sec=?6,access_nsec=?7,modify_sec=?8,modify_nsec=?9,change_sec=?10,change_nsec=?11 WHERE object=?12",
        params![uid,gid,mode,b.seconds,b.nanoseconds,a.seconds,a.nanoseconds,m.seconds,m.nanoseconds,time.seconds,time.nanoseconds,id.raw()])?;
    if changed != 1 {
        return Err(Error::Corrupt(
            "missing object metadata during setattr".into(),
        ));
    }
    Ok(())
}

impl Volume {
    pub(crate) fn require_metadata(&self) -> Result<()> {
        if self.catalog_version != CatalogVersion::MetadataV2 {
            return Err(Error::UnsupportedOperation);
        }
        Ok(())
    }
    pub fn volume_identity(&self) -> Result<VolumeIdentity> {
        self.require_metadata()?;
        self.observe(identity(&self.db))
    }
    pub fn metadata(&self, id: FileId) -> Result<Metadata> {
        self.require_metadata()?;
        self.stat(id)?;
        self.observe(get(&self.db, id))
    }
    /// Trusted core API only: adapters must authorize ownership/mode/time changes.
    /// No automatic access-time updates yet. Empty updates do not change ctime.
    pub fn set_metadata(&mut self, id: FileId, update: MetadataUpdate) -> Result<()> {
        self.require_metadata()?;
        validate_update(update)?;
        self.mutate(|this| {
            let stat = this.stat(id)?;
            if update == MetadataUpdate::default() {
                return Ok(());
            }
            let c = Timestamp::now()?;
            let before = this.take_fault(1);
            let tx = this.db.transaction()?;
            apply_update(&tx, this.catalog_version, id, stat.kind, update, c, false)?;
            tx.execute(
                "UPDATE objects SET generation=generation+1 WHERE id=?1",
                [id.raw()],
            )?;
            Self::interrupt(before)?;
            tx.commit()?;
            Ok(())
        })
    }
}
