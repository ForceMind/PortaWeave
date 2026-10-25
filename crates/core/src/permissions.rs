//! Mode-only policy over a serialized v2 owner. Credentials are TRUSTED INPUT,
//! not authentication of an OS caller. Raw Volume APIs remain administrative.
use crate::{
    CreateAttributes, Entry, Error, FileId, Handle, Kind, MaintenanceStatus, Metadata,
    MetadataUpdate, ROOT, ReadView, Result, Stat, Volume, WritePosition, metadata,
};
use std::collections::BTreeSet;

pub const MAX_SUPPLEMENTARY_GROUPS: usize = 128;
pub const MAX_POSIX_FILES: usize = 4096;
const MAX_SEARCH_DIRECTORIES: usize = 256;

/// Effective IDs supplied by an authenticated adapter. No Default/root fallback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Credentials {
    uid: u32,
    gid: u32,
    groups: Vec<u32>,
}
impl Credentials {
    pub fn new(uid: u32, gid: u32, supplementary_groups: &[u32]) -> Result<Self> {
        if supplementary_groups.len() > MAX_SUPPLEMENTARY_GROUPS {
            return Err(Error::ResourceLimit);
        }
        let mut groups = supplementary_groups.to_vec();
        groups.sort_unstable();
        groups.dedup();
        Ok(Self { uid, gid, groups })
    }
    pub fn uid(&self) -> u32 {
        self.uid
    }
    pub fn gid(&self) -> u32 {
        self.gid
    }
    pub fn supplementary_groups(&self) -> &[u32] {
        &self.groups
    }
    fn in_group(&self, group: u32) -> bool {
        self.gid == group || self.groups.binary_search(&group).is_ok()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Access(u8);
impl Access {
    pub const EXISTS: Self = Self(0);
    pub const READ: Self = Self(4);
    pub const WRITE: Self = Self(2);
    /// Directory search and ordinary-file execution use the same mode bit.
    pub const EXECUTE: Self = Self(1);
    pub const fn bits(self) -> u8 {
        self.0
    }
    pub fn from_bits(bits: u8) -> Result<Self> {
        if bits & !7 != 0 {
            return Err(Error::InvalidRange);
        }
        Ok(Self(bits))
    }
}
impl std::ops::BitOr for Access {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileAccess {
    Read,
    Write,
    ReadWrite,
}
impl FileAccess {
    fn rights(self) -> Access {
        match self {
            Self::Read => Access::READ,
            Self::Write => Access::WRITE,
            Self::ReadWrite => Access::READ | Access::WRITE,
        }
    }
}

/// One serialized observation of a granted v2 file; not an OS cache promise.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileInfo {
    pub stat: Stat,
    pub metadata: Metadata,
}
impl FileInfo {
    fn at(db: &rusqlite::Connection, id: FileId) -> Result<Self> {
        Ok(Self {
            stat: Volume::stat_at(db, id)?,
            metadata: metadata::get(db, id)?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileWriteResult {
    pub written: usize,
    /// Resolved absolute position, including for append and zero-byte results.
    pub offset: u64,
    pub info: FileInfo,
}

/// Granted descriptor capability, not a path or an authenticated caller token.
/// Explicit close is required; copies become invalid together after close.
#[must_use = "close the granted file explicitly to release its open-object reference"]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PosixFile {
    handle: Handle,
    rights: Access,
}
impl PosixFile {
    fn object(self, volume: &Volume, access: Access) -> Result<FileId> {
        let id = volume.object(self.handle)?;
        if self.rights.0 & access.0 != access.0 {
            return Err(Error::PermissionDenied);
        }
        Ok(id)
    }
    /// Granted read/write rights survive later chmod/chown/unlink/rename.
    pub fn read(self, volume: &Volume, offset: u64, len: usize) -> Result<Vec<u8>> {
        self.object(volume, Access::READ)?;
        volume.read(self.handle, offset, len)
    }
    pub fn write(self, volume: &mut Volume, offset: u64, data: &[u8]) -> Result<usize> {
        self.object(volume, Access::WRITE)?;
        volume.write(self.handle, offset, data)
    }
    /// Atomic append/offset/constrained data request. The committed reply is
    /// assembled in the business transaction, before its commit/maintenance.
    pub fn write_request(
        self,
        volume: &mut Volume,
        position: WritePosition,
        data: &[u8],
        constrained: bool,
    ) -> Result<FileWriteResult> {
        self.write_request_with_reply(volume, position, data, constrained, Ok)
    }
    /// A pure, fallible platform projection runs before commit. It must not
    /// perform external side effects; rejection rolls back the business change.
    pub fn write_request_with_reply<T>(
        self,
        volume: &mut Volume,
        position: WritePosition,
        data: &[u8],
        constrained: bool,
        reply: impl FnOnce(FileWriteResult) -> Result<T>,
    ) -> Result<T> {
        self.object(volume, Access::WRITE)?;
        volume.write_request(
            self.handle,
            position,
            data,
            constrained,
            |db, id, written, offset| {
                reply(FileWriteResult {
                    written,
                    offset,
                    info: FileInfo::at(db, id)?,
                })
            },
        )
    }
    pub fn info(self, volume: &Volume) -> Result<FileInfo> {
        let id = self.object(volume, Access::EXISTS)?;
        volume.observe(FileInfo::at(&volume.db, id))
    }
    pub fn truncate_request(self, volume: &mut Volume, size: u64) -> Result<FileInfo> {
        self.truncate_request_with_reply(volume, size, Ok)
    }
    /// Pure platform reply construction, under the same pre-commit contract.
    pub fn truncate_request_with_reply<T>(
        self,
        volume: &mut Volume,
        size: u64,
        reply: impl FnOnce(FileInfo) -> Result<T>,
    ) -> Result<T> {
        self.object(volume, Access::WRITE)?;
        volume.truncate_request(self.handle, size, MetadataUpdate::default(), |db, id| {
            reply(FileInfo::at(db, id)?)
        })
    }
    pub fn truncate(self, volume: &mut Volume, size: u64) -> Result<()> {
        self.object(volume, Access::WRITE)?;
        volume.truncate(self.handle, size)
    }
    pub fn stat(self, volume: &Volume) -> Result<Stat> {
        volume.stat(self.object(volume, Access::EXISTS)?)
    }
    pub fn metadata(self, volume: &Volume) -> Result<Metadata> {
        volume.metadata(self.object(volume, Access::EXISTS)?)
    }
    pub fn close(self, volume: &mut Volume) -> Result<()> {
        self.object(volume, Access::EXISTS)?;
        volume.close_file(self.handle)
    }
    pub fn acquire_range_view(
        self,
        volume: &mut Volume,
        offset: u64,
        length: u64,
    ) -> Result<PosixReadView> {
        self.object(volume, Access::READ)?;
        Ok(PosixReadView {
            view: volume.acquire_range_view(self.handle, offset, length)?,
        })
    }
}

/// Immutable read grant over one admitted absolute range; not native mmap.
#[must_use = "close the view explicitly to release its retained block mappings"]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PosixReadView {
    view: ReadView,
}
impl PosixReadView {
    pub fn read(self, volume: &Volume, offset: u64, len: usize) -> Result<Vec<u8>> {
        volume.read_view(self.view, offset, len)
    }
    pub fn close(self, volume: &mut Volume) -> Result<()> {
        volume.release_view(self.view)
    }
}

fn mode_access(caller: &Credentials, metadata: Metadata, kind: Kind, access: Access) -> Result<()> {
    if caller.uid == 0 {
        // This profile models uid 0 privilege explicitly, never host process UID.
        if kind == Kind::File && access.0 & 1 != 0 && metadata.mode & 0o111 == 0 {
            return Err(Error::PermissionDenied);
        }
        return Ok(());
    }
    let shift = if caller.uid == metadata.uid {
        6
    } else if caller.in_group(metadata.gid) {
        3
    } else {
        0
    };
    let granted = (metadata.mode >> shift) as u8 & 7;
    if granted & access.0 != access.0 {
        return Err(Error::PermissionDenied);
    }
    Ok(())
}

/// A synchronous policy facade. Its exclusive borrow prevents interleaving a
/// metadata/namespace change between checks and the existing core transaction.
/// It is not an ACL implementation, OS authentication or a backing-file sandbox.
pub struct PosixSession<'v> {
    volume: &'v mut Volume,
    caller: Credentials,
}
impl Volume {
    pub fn posix(&mut self, caller: Credentials) -> Result<PosixSession<'_>> {
        self.require_metadata()?;
        Ok(PosixSession {
            volume: self,
            caller,
        })
    }
}
impl PosixSession<'_> {
    /// Search every ancestor and the selected directory, bounded at Core ROOT.
    /// No symlinks or host paths are interpreted by this component-name API.
    fn search(&self, directory: FileId) -> Result<()> {
        let mut cursor = directory;
        let mut seen = BTreeSet::new();
        for _ in 0..MAX_SEARCH_DIRECTORIES {
            self.volume.directory(cursor)?;
            if !seen.insert(cursor) {
                return self
                    .volume
                    .observe(Err(Error::Corrupt("permission ancestor cycle".into())));
            }
            mode_access(
                &self.caller,
                self.volume.metadata(cursor)?,
                Kind::Directory,
                Access::EXECUTE,
            )?;
            if cursor == ROOT {
                return Ok(());
            }
            cursor = self.volume.parent_directory(cursor)?;
        }
        Err(Error::ResourceLimit)
    }
    fn directory_access(&self, directory: FileId, access: Access) -> Result<Metadata> {
        self.search(directory)?;
        let metadata = self.volume.metadata(directory)?;
        mode_access(&self.caller, metadata, Kind::Directory, access)?;
        Ok(metadata)
    }
    fn sticky(&self, directory: Metadata, object: FileId) -> Result<()> {
        if directory.mode & 0o1000 != 0
            && self.caller.uid != 0
            && self.caller.uid != directory.uid
            && self.caller.uid != self.volume.metadata(object)?.uid
        {
            return Err(Error::PermissionDenied);
        }
        Ok(())
    }
    pub fn lookup(&self, parent: FileId, name: &str) -> Result<Entry> {
        self.search(parent)?;
        self.volume.lookup_entry(parent, name)
    }
    pub fn stat(&self, parent: FileId, name: &str) -> Result<Stat> {
        self.volume.stat(self.lookup(parent, name)?.id)
    }
    pub fn metadata(&self, parent: FileId, name: &str) -> Result<Metadata> {
        self.volume.metadata(self.lookup(parent, name)?.id)
    }
    /// Effective-ID mode calculation. Not libc access(2) real-ID semantics.
    pub fn check_access(&self, parent: FileId, name: &str, access: Access) -> Result<()> {
        let id = self.lookup(parent, name)?.id;
        mode_access(
            &self.caller,
            self.volume.metadata(id)?,
            self.volume.stat(id)?.kind,
            access,
        )
    }
    pub fn list_bounded(&self, parent: FileId, maximum: usize) -> Result<Vec<Entry>> {
        self.directory_access(parent, Access::READ | Access::EXECUTE)?;
        self.volume.list_bounded(parent, maximum)
    }
    pub fn open_file(
        &mut self,
        parent: FileId,
        name: &str,
        access: FileAccess,
    ) -> Result<PosixFile> {
        let id = self.lookup(parent, name)?.id;
        if self.volume.stat(id)?.kind != Kind::File {
            return Err(Error::IsDirectory);
        }
        let rights = access.rights();
        mode_access(&self.caller, self.volume.metadata(id)?, Kind::File, rights)?;
        if self.volume.open_handle_count() >= MAX_POSIX_FILES {
            return Err(Error::ResourceLimit);
        }
        let handle = self.volume.open_file(id)?;
        Ok(PosixFile { handle, rights })
    }
    fn metadata_update(&self, id: FileId, mut update: MetadataUpdate) -> Result<MetadataUpdate> {
        metadata::validate_update(update)?;
        if update == MetadataUpdate::default() {
            return Ok(update);
        }
        let old = self.volume.metadata(id)?;
        if self.caller.uid != 0 {
            if self.caller.uid != old.uid
                || update.uid.is_some_and(|uid| uid != old.uid)
                || update.gid.is_some_and(|gid| !self.caller.in_group(gid))
            {
                return Err(Error::PermissionDenied);
            }
            let gid = update.gid.unwrap_or(old.gid);
            if let Some(mode) = update.mode.as_mut()
                && !self.caller.in_group(gid)
            {
                *mode &= !0o2000;
            }
        }
        // This profile clears file setid on any explicit ownership request,
        // including same-ID requests; raw administrative semantics stay unchanged.
        if self.volume.stat(id)?.kind == Kind::File
            && (update.uid.is_some() || update.gid.is_some())
        {
            update.mode = Some(update.mode.unwrap_or(old.mode) & 0o1777);
        }
        Ok(update)
    }
    pub fn set_metadata(
        &mut self,
        parent: FileId,
        name: &str,
        update: MetadataUpdate,
    ) -> Result<()> {
        let id = self.lookup(parent, name)?.id;
        let update = self.metadata_update(id, update)?;
        self.volume.set_metadata(id, update)
    }
    pub fn truncate_with_metadata(
        &mut self,
        file: PosixFile,
        size: u64,
        update: MetadataUpdate,
    ) -> Result<()> {
        let id = file.object(self.volume, Access::WRITE)?;
        let update = self.metadata_update(id, update)?;
        self.volume
            .truncate_with_metadata(file.handle, size, update)
    }
    /// Current metadata authority on a live granted inode, independent of its
    /// data rights or current pathname. Reply is captured before commit.
    pub fn set_file_metadata_request(
        &mut self,
        file: PosixFile,
        update: MetadataUpdate,
    ) -> Result<FileInfo> {
        self.set_file_metadata_with_reply(file, update, Ok)
    }
    /// Pure platform projection; refusal rolls back the metadata transaction.
    /// A data grant is not a grant to change metadata: current owner/group
    /// checks still apply, including after rename/unlink or chown.
    pub fn set_file_metadata_with_reply<T>(
        &mut self,
        file: PosixFile,
        update: MetadataUpdate,
        reply: impl FnOnce(FileInfo) -> Result<T>,
    ) -> Result<T> {
        let id = file.object(self.volume, Access::EXISTS)?;
        let update = self.metadata_update(id, update)?;
        self.volume
            .set_metadata_request(id, update, |db, _, id| reply(FileInfo::at(db, id)?))
    }
    fn creation_attributes(
        &self,
        parent: FileId,
        directory: bool,
        mode: u16,
        umask: u16,
    ) -> Result<CreateAttributes> {
        // File creation accepts permission bits; mkdir also accepts sticky.
        // setuid/setgid are not accepted as caller-supplied creation flags here.
        if mode & !(if directory { 0o1777 } else { 0o777 }) != 0 || umask & !0o777 != 0 {
            return Err(Error::InvalidRange);
        }
        let parent = self.directory_access(parent, Access::WRITE | Access::EXECUTE)?;
        let inherits_group = parent.mode & 0o2000 != 0;
        let permissions = (mode & 0o777) & !umask;
        Ok(CreateAttributes {
            uid: self.caller.uid,
            gid: if inherits_group {
                parent.gid
            } else {
                self.caller.gid
            },
            mode: permissions
                | if directory {
                    (mode & 0o1000) | if inherits_group { 0o2000 } else { 0 }
                } else {
                    0
                },
        })
    }
    pub fn create_file(
        &mut self,
        parent: FileId,
        name: &str,
        mode: u16,
        umask: u16,
    ) -> Result<FileId> {
        let attributes = self.creation_attributes(parent, false, mode, umask)?;
        self.volume
            .create_file_with_attributes(parent, name, attributes)
    }
    pub fn mkdir(&mut self, parent: FileId, name: &str, mode: u16, umask: u16) -> Result<FileId> {
        let attributes = self.creation_attributes(parent, true, mode, umask)?;
        self.volume.mkdir_with_attributes(parent, name, attributes)
    }
    pub fn unlink(&mut self, parent: FileId, name: &str) -> Result<()> {
        let directory = self.directory_access(parent, Access::WRITE | Access::EXECUTE)?;
        let object = self.volume.lookup(parent, name)?;
        self.sticky(directory, object)?;
        self.volume.unlink(parent, name)
    }
    pub fn rmdir(&mut self, parent: FileId, name: &str) -> Result<()> {
        let directory = self.directory_access(parent, Access::WRITE | Access::EXECUTE)?;
        let object = self.volume.lookup(parent, name)?;
        self.sticky(directory, object)?;
        self.volume.rmdir(parent, name)
    }
    pub fn rename(
        &mut self,
        from_parent: FileId,
        from: &str,
        to_parent: FileId,
        to: &str,
        replace: bool,
    ) -> Result<()> {
        let source_parent = self.directory_access(from_parent, Access::WRITE | Access::EXECUTE)?;
        let destination_parent =
            self.directory_access(to_parent, Access::WRITE | Access::EXECUTE)?;
        let source = self.volume.lookup(from_parent, from)?;
        self.sticky(source_parent, source)?;
        match self.volume.lookup(to_parent, to) {
            Ok(target) => self.sticky(destination_parent, target)?,
            Err(Error::NotFound) => (),
            Err(error) => return Err(error),
        }
        self.volume
            .rename(from_parent, from, to_parent, to, replace)
    }
    pub fn maintenance_status(&self) -> &MaintenanceStatus {
        self.volume.maintenance_status()
    }
    pub fn is_write_protected(&self) -> bool {
        self.volume.is_write_protected()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Timestamp;
    #[test]
    fn exhaustive_mode_classes_do_not_fall_back_to_more_permissive_classes() {
        let t = Timestamp {
            seconds: 0,
            nanoseconds: 0,
        };
        for owner in 0_u16..8 {
            for group in 0_u16..8 {
                for other in 0_u16..8 {
                    let m = Metadata {
                        uid: 10,
                        gid: 20,
                        mode: owner * 64 + group * 8 + other,
                        created: t,
                        accessed: t,
                        modified: t,
                        changed: t,
                    };
                    let callers = [
                        (Credentials::new(10, 20, &[]).unwrap(), owner),
                        (Credentials::new(11, 20, &[]).unwrap(), group),
                        (Credentials::new(11, 30, &[20]).unwrap(), group),
                        (Credentials::new(11, 30, &[]).unwrap(), other),
                    ];
                    for (caller, expected) in callers {
                        for bits in 0..8 {
                            let allowed = (u16::from(bits) & expected) == u16::from(bits);
                            for kind in [Kind::File, Kind::Directory] {
                                assert_eq!(
                                    mode_access(&caller, m, kind, Access::from_bits(bits).unwrap())
                                        .is_ok(),
                                    allowed
                                );
                            }
                        }
                    }
                    let root = Credentials::new(0, 99, &[]).unwrap();
                    for bits in 0..8 {
                        let access = Access::from_bits(bits).unwrap();
                        assert!(mode_access(&root, m, Kind::Directory, access).is_ok());
                        assert_eq!(
                            mode_access(&root, m, Kind::File, access).is_ok(),
                            bits & 1 == 0 || m.mode & 0o111 != 0
                        );
                    }
                }
            }
        }
    }
}
