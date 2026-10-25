//! Mode-only policy over a serialized v2 owner. Credentials are TRUSTED INPUT,
//! not authentication of an OS caller. Raw Volume APIs remain administrative.
use crate::{
    CreateAttributes, Entry, Error, FileId, Kind, MaintenanceStatus, Metadata, ROOT, Result, Stat,
    Volume,
};
use std::collections::BTreeSet;

pub const MAX_SUPPLEMENTARY_GROUPS: usize = 128;
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
