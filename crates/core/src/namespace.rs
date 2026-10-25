use crate::{
    CreateAttributes, Entry, Error, FileId, Handle, Kind, MetadataUpdate, ObjectInfo, ROOT, Result,
    Timestamp, Volume, catalog::CatalogVersion, metadata, names,
};
use rusqlite::{OptionalExtension, params};

/// Trusted initial state. File size is logical/sparse, not reserved bytes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NodeCreateOptions {
    pub attributes: Option<CreateAttributes>,
    pub size: Option<u64>,
    pub created: Option<Timestamp>,
    pub accessed: Option<Timestamp>,
    pub modified: Option<Timestamp>,
}

/// A completed trusted create. Close the administrative pin explicitly.
#[must_use = "close any returned file handle explicitly"]
#[derive(Debug)]
pub struct CreatedNode<T> {
    pub object: FileId,
    pub handle: Option<Handle>,
    pub reply: T,
}

/// Trusted cached identities, not authorization. None requires an absent
/// destination, except a case-only rename of the very same directory entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenameExpectation {
    pub source: FileId,
    pub target: Option<FileId>,
}

fn check_rename(
    source: Option<FileId>,
    target: Option<FileId>,
    same_entry: bool,
    expected: RenameExpectation,
) -> Result<()> {
    if source != Some(expected.source)
        || if same_entry {
            expected.target.is_some_and(|id| id != expected.source)
        } else {
            target != expected.target
        }
    {
        return Err(Error::StaleEntry);
    }
    Ok(())
}

impl Volume {
    fn lookup_key(&self, parent: FileId, key: &str) -> Result<Option<FileId>> {
        Self::lookup_key_at(&self.db, parent, key)
    }
    fn lookup_key_at(
        db: &rusqlite::Connection,
        parent: FileId,
        key: &str,
    ) -> Result<Option<FileId>> {
        Ok(db
            .query_row(
                "SELECT object FROM entries WHERE parent=?1 AND name_key=?2",
                params![parent.0, key],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            .map(FileId))
    }
    pub fn lookup(&self, parent: FileId, name: &str) -> Result<FileId> {
        Ok(self.lookup_entry(parent, name)?.id)
    }
    /// Return the stored spelling without reimplementing name comparison in
    /// native adapters. Hard-linked aliases may share an ID but not a name.
    pub fn lookup_entry(&self, parent: FileId, name: &str) -> Result<Entry> {
        self.observe((|| {
            self.directory(parent)?;
            self.db
                .query_row(
                    "SELECT display_name,object FROM entries WHERE parent=?1 AND name_key=?2",
                    params![parent.0, names::key(name)?],
                    |row| {
                        Ok(Entry {
                            name: row.get(0)?,
                            id: FileId(row.get(1)?),
                        })
                    },
                )
                .optional()?
                .ok_or(Error::NotFound)
        })())
    }
    /// A directory's logical parent, bounded at this volume's own root.
    pub fn parent_directory(&self, directory: FileId) -> Result<FileId> {
        self.observe((|| {
            self.directory(directory)?;
            if directory == ROOT {
                return Ok(ROOT);
            }
            self.db
                .query_row(
                    "SELECT parent FROM entries WHERE object=?1",
                    [directory.0],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?
                .map(FileId)
                .ok_or(Error::NotFound)
        })())
    }
    /// Directory snapshot cap checked before materializing all names.
    /// The sole Volume owner serializes catalog modifications.
    pub fn list_bounded(&self, parent: FileId, maximum: usize) -> Result<Vec<Entry>> {
        self.observe((|| {
            self.directory(parent)?;
            let count: i64 = self.db.query_row(
                "SELECT COUNT(*) FROM entries WHERE parent=?1",
                [parent.0],
                |row| row.get(0),
            )?;
            let count = usize::try_from(count).map_err(|_| Error::ResourceLimit)?;
            if count > maximum {
                return Err(Error::ResourceLimit);
            }
            self.list(parent)
        })())
    }
    pub fn list(&self, parent: FileId) -> Result<Vec<Entry>> {
        self.observe((|| {
            self.directory(parent)?;
            let mut statement = self.db.prepare(
                "SELECT display_name,object FROM entries WHERE parent=?1 ORDER BY name_key",
            )?;
            Ok(statement
                .query_map([parent.0], |r| {
                    Ok(Entry {
                        name: r.get(0)?,
                        id: FileId(r.get(1)?),
                    })
                })?
                .collect::<std::result::Result<_, _>>()?)
        })())
    }
    fn create_node(
        &mut self,
        parent: FileId,
        name: &str,
        kind: i64,
        attributes: Option<CreateAttributes>,
    ) -> Result<FileId> {
        self.create_node_request(
            parent,
            name,
            kind,
            NodeCreateOptions {
                attributes,
                ..Default::default()
            },
            None,
            |_, _, id| Ok(id),
        )
    }
    /// Trusted create with a reserved file pin and pure, pre-commit projection.
    /// v1/v2 defaults remain unchanged. Not OS caller authentication.
    pub fn create_node_with_reply<T>(
        &mut self,
        parent: FileId,
        name: &str,
        kind: Kind,
        attributes: Option<CreateAttributes>,
        reply: impl FnOnce(ObjectInfo) -> Result<T>,
    ) -> Result<CreatedNode<T>> {
        self.create_node_with_options(
            parent,
            name,
            kind,
            NodeCreateOptions {
                attributes,
                ..Default::default()
            },
            reply,
        )
    }
    /// Create initial file size and v2 times in the same namespace transaction.
    pub fn create_node_with_options<T>(
        &mut self,
        parent: FileId,
        name: &str,
        kind: Kind,
        options: NodeCreateOptions,
        reply: impl FnOnce(ObjectInfo) -> Result<T>,
    ) -> Result<CreatedNode<T>> {
        let handle = if kind == Kind::File {
            Some(self.reserve_file_handle()?)
        } else {
            None
        };
        self.create_node_request(
            parent,
            name,
            if kind == Kind::File { 0 } else { 1 },
            options,
            handle,
            |db, version, id| {
                Ok(CreatedNode {
                    object: id,
                    handle,
                    reply: reply(ObjectInfo::at(db, version, id)?)?,
                })
            },
        )
    }
    fn create_node_request<T>(
        &mut self,
        parent: FileId,
        name: &str,
        kind: i64,
        options: NodeCreateOptions,
        handle: Option<Handle>,
        reply: impl FnOnce(&rusqlite::Connection, CatalogVersion, FileId) -> Result<T>,
    ) -> Result<T> {
        if kind == 1 && options.size.is_some() {
            return Err(Error::UnsupportedOperation);
        }
        let initial_times = MetadataUpdate {
            created: options.created,
            accessed: options.accessed,
            modified: options.modified,
            ..Default::default()
        };
        metadata::validate_update(initial_times)?;
        if initial_times != MetadataUpdate::default() {
            self.require_metadata()?;
        }
        if let Some(a) = options.attributes {
            self.require_metadata()?;
            a.validate()?;
        }
        self.mutate(|this| {
            this.directory(parent)?;
            let key = names::key(name)?;
            if let Some(size) = options.size {
                this.quota(0, size)?;
            }
            if this.lookup_key(parent, &key)?.is_some() {
                return Err(Error::Exists);
            }
            let attributes = if this.catalog_version == CatalogVersion::MetadataV2 {
                let parent_attrs = this.metadata(parent)?;
                Some(options.attributes.unwrap_or(CreateAttributes {
                    uid: parent_attrs.uid,
                    gid: parent_attrs.gid,
                    mode: if kind == 1 { 0o700 } else { 0o600 },
                }))
            } else {
                None
            };
            let time = Timestamp::now()?;
            let before = this.take_fault(1);
            let tx = this.db.transaction()?;
            tx.execute(
                "INSERT INTO objects(kind,size) VALUES (?1,?2)",
                params![kind, options.size.unwrap_or(0) as i64],
            )?;
            let id = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO entries VALUES (?1,?2,?3,?4)",
                params![parent.0, key, name, id],
            )?;
            if let Some(a) = attributes {
                metadata::insert(&tx, FileId(id), a, time)?;
                if initial_times != MetadataUpdate::default() {
                    metadata::apply_update(
                        &tx,
                        this.catalog_version,
                        FileId(id),
                        if kind == 0 {
                            Kind::File
                        } else {
                            Kind::Directory
                        },
                        initial_times,
                        time,
                        false,
                    )?;
                }
            }
            metadata::touch(&tx, this.catalog_version, parent, time, true, false)?;
            let result = reply(&tx, this.catalog_version, FileId(id))?;
            Self::interrupt(before)?;
            tx.commit()?;
            if let Some(handle) = handle {
                this.publish_file_handle(handle, FileId(id));
            }
            Ok(result)
        })
    }
    pub fn create_file(&mut self, parent: FileId, name: &str) -> Result<FileId> {
        self.create_node(parent, name, 0, None)
    }
    pub fn mkdir(&mut self, parent: FileId, name: &str) -> Result<FileId> {
        self.create_node(parent, name, 1, None)
    }
    /// Trusted adapter entry point. Caller supplies already-authorized attributes.
    pub fn create_file_with_attributes(
        &mut self,
        parent: FileId,
        name: &str,
        attributes: CreateAttributes,
    ) -> Result<FileId> {
        self.create_node(parent, name, 0, Some(attributes))
    }
    pub fn mkdir_with_attributes(
        &mut self,
        parent: FileId,
        name: &str,
        attributes: CreateAttributes,
    ) -> Result<FileId> {
        self.create_node(parent, name, 1, Some(attributes))
    }

    /// A second directory entry for the SAME object, not copied content.
    pub fn hard_link(&mut self, object: FileId, parent: FileId, name: &str) -> Result<()> {
        self.mutate(|this| {
            if this.stat(object)?.kind != Kind::File {
                return Err(Error::IsDirectory);
            }
            this.directory(parent)?;
            let key = names::key(name)?;
            if this.lookup_key(parent, &key)?.is_some() {
                return Err(Error::Exists);
            }
            let time = Timestamp::now()?;
            let tx = this.db.transaction()?;
            tx.execute(
                "INSERT INTO entries VALUES (?1,?2,?3,?4)",
                params![parent.0, key, name, object.0],
            )?;
            metadata::touch(&tx, this.catalog_version, parent, time, true, false)?;
            metadata::touch(&tx, this.catalog_version, object, time, false, false)?;
            tx.commit()?;
            Ok(())
        })
    }
    pub fn unlink(&mut self, parent: FileId, name: &str) -> Result<()> {
        self.remove_request(parent, name, Kind::File, None)
    }
    pub fn rmdir(&mut self, parent: FileId, name: &str) -> Result<()> {
        self.remove_request(parent, name, Kind::Directory, None)
    }
    /// Trusted removal, checking the cached inode in the same owner request
    /// and again in the SQL transaction. Does not authenticate the caller.
    pub fn remove_checked(
        &mut self,
        parent: FileId,
        name: &str,
        kind: Kind,
        expected: FileId,
    ) -> Result<()> {
        self.remove_request(parent, name, kind, Some(expected))
    }
    fn remove_request(
        &mut self,
        parent: FileId,
        name: &str,
        kind: Kind,
        expected: Option<FileId>,
    ) -> Result<()> {
        self.mutate(|this| {
            this.directory(parent)?;
            let key = names::key(name)?;
            let found = this.lookup_key(parent, &key)?;
            if expected.is_some() && found != expected {
                return Err(Error::StaleEntry);
            }
            let id = found.ok_or(Error::NotFound)?;
            let actual = this.stat(id)?.kind;
            if actual != kind {
                return Err(if actual == Kind::Directory {
                    Error::IsDirectory
                } else {
                    Error::NotDirectory
                });
            }
            if kind == Kind::Directory && !this.list(id)?.is_empty() {
                return Err(Error::NotEmpty);
            }
            let time = Timestamp::now()?;
            let before = this.take_fault(1);
            let tx = this.db.transaction()?;
            if expected.is_some() && Self::lookup_key_at(&tx, parent, &key)? != expected {
                return Err(Error::StaleEntry);
            }
            tx.execute(
                "DELETE FROM entries WHERE parent=?1 AND name_key=?2",
                params![parent.0, key],
            )?;
            if kind == Kind::Directory {
                tx.execute("DELETE FROM objects WHERE id=?1", [id.0])?;
            } else {
                metadata::touch(&tx, this.catalog_version, id, time, false, false)?;
            }
            metadata::touch(&tx, this.catalog_version, parent, time, true, false)?;
            Self::interrupt(before)?;
            tx.commit()?;
            Ok(())
        })
    }

    /// POSIX-style namespace replacement in one SQLite transaction. Windows
    /// sharing/deletion rules must be enforced by a future platform adapter.
    pub fn rename(
        &mut self,
        from_parent: FileId,
        from: &str,
        to_parent: FileId,
        to: &str,
        replace: bool,
    ) -> Result<()> {
        self.rename_request(from_parent, from, to_parent, to, replace, None)
    }
    /// Compare source and destination identities before replacing either name.
    /// Expected target None requires absence; case-only same-entry rename is
    /// allowed. A distinct hard-link alias is still a distinct directory entry.
    pub fn rename_checked(
        &mut self,
        from_parent: FileId,
        from: &str,
        to_parent: FileId,
        to: &str,
        expected: RenameExpectation,
    ) -> Result<()> {
        self.rename_request(
            from_parent,
            from,
            to_parent,
            to,
            expected.target.is_some(),
            Some(expected),
        )
    }
    fn rename_request(
        &mut self,
        from_parent: FileId,
        from: &str,
        to_parent: FileId,
        to: &str,
        replace: bool,
        expected: Option<RenameExpectation>,
    ) -> Result<()> {
        self.mutate(|this| {
            this.directory(from_parent)?;
            this.directory(to_parent)?;
            let old_key = names::key(from)?;
            let new_key = names::key(to)?;
            let source = this.lookup_key(from_parent, &old_key)?;
            let target = this.lookup_key(to_parent, &new_key)?;
            let same_entry = from_parent == to_parent && old_key == new_key;
            if let Some(expected) = expected {
                check_rename(source, target, same_entry, expected)?;
            }
            let source = source.ok_or(Error::NotFound)?;
            if same_entry {
                let time = Timestamp::now()?;
                let before = this.take_fault(1);
                let tx = this.db.transaction()?;
                if let Some(expected) = expected {
                    check_rename(Self::lookup_key_at(&tx, from_parent, &old_key)?, Self::lookup_key_at(&tx, to_parent, &new_key)?, true, expected)?;
                }
                tx.execute("UPDATE entries SET display_name=?1 WHERE parent=?2 AND name_key=?3", params![to, from_parent.0, old_key])?;
                metadata::touch(&tx, this.catalog_version, from_parent, time, true, false)?;
                metadata::touch(&tx, this.catalog_version, source, time, false, false)?;
                Self::interrupt(before)?;
                tx.commit()?;
                return Ok(());
            }
            if target.is_some() && !replace { return Err(Error::Exists); }
            // rename(a,b) where a and b are two hard links is a no-op.
            if target == Some(source) { return Ok(()); }
            let source_kind = this.stat(source)?.kind;
            if source_kind == Kind::Directory {
                let mut cursor = to_parent;
                let mut seen = std::collections::BTreeSet::new();
                loop {
                    if cursor == source { return Err(Error::Cycle); }
                    if cursor == ROOT { break; }
                    if !seen.insert(cursor) { return Err(Error::Corrupt("ancestor cycle".into())); }
                    cursor = FileId(this.db.query_row("SELECT parent FROM entries WHERE object=?1", [cursor.0], |r| r.get(0))?);
                }
            }
            if let Some(id) = target {
                let kind = this.stat(id)?.kind;
                if kind != source_kind {
                    return Err(if kind == Kind::Directory { Error::IsDirectory } else { Error::NotDirectory });
                }
                if kind == Kind::Directory && !this.list(id)?.is_empty() { return Err(Error::NotEmpty); }
            }
            let time = Timestamp::now()?;
            let before = this.take_fault(1);
            let tx = this.db.transaction()?;
            if let Some(expected) = expected {
                check_rename(Self::lookup_key_at(&tx, from_parent, &old_key)?, Self::lookup_key_at(&tx, to_parent, &new_key)?, false, expected)?;
            }
            if let Some(id) = target {
                tx.execute("DELETE FROM entries WHERE parent=?1 AND name_key=?2", params![to_parent.0, new_key])?;
                if source_kind == Kind::Directory { tx.execute("DELETE FROM objects WHERE id=?1", [id.0])?; }
                else { metadata::touch(&tx, this.catalog_version, id, time, false, false)?; }
            }
            let changed = tx.execute("UPDATE entries SET parent=?1,name_key=?2,display_name=?3 WHERE parent=?4 AND name_key=?5",
                params![to_parent.0, new_key, to, from_parent.0, old_key])?;
            if changed != 1 { return Err(Error::Corrupt("lost source during rename".into())); }
            metadata::touch(&tx, this.catalog_version, from_parent, time, true, false)?;
            if from_parent != to_parent { metadata::touch(&tx, this.catalog_version, to_parent, time, true, false)?; }
            metadata::touch(&tx, this.catalog_version, source, time, false, false)?;
            Self::interrupt(before)?;
            tx.commit()?;
            Ok(())
        })
    }
}
