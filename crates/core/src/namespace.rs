use crate::{
    CreateAttributes, Entry, Error, FileId, Kind, ROOT, Result, Timestamp, Volume,
    catalog::CatalogVersion, metadata, names,
};
use rusqlite::{OptionalExtension, params};

impl Volume {
    fn lookup_key(&self, parent: FileId, key: &str) -> Result<Option<FileId>> {
        Ok(self
            .db
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
        if let Some(a) = attributes {
            self.require_metadata()?;
            a.validate()?;
        }
        self.mutate(|this| {
            this.directory(parent)?;
            let key = names::key(name)?;
            if this.lookup_key(parent, &key)?.is_some() {
                return Err(Error::Exists);
            }
            let attributes = if this.catalog_version == CatalogVersion::MetadataV2 {
                let parent_attrs = this.metadata(parent)?;
                Some(attributes.unwrap_or(CreateAttributes {
                    uid: parent_attrs.uid,
                    gid: parent_attrs.gid,
                    mode: if kind == 1 { 0o700 } else { 0o600 },
                }))
            } else {
                None
            };
            let time = Timestamp::now()?;
            let tx = this.db.transaction()?;
            tx.execute("INSERT INTO objects(kind) VALUES (?1)", [kind])?;
            let id = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO entries VALUES (?1,?2,?3,?4)",
                params![parent.0, key, name, id],
            )?;
            if let Some(a) = attributes {
                metadata::insert(&tx, FileId(id), a, time)?;
            }
            metadata::touch(&tx, this.catalog_version, parent, time, true, false)?;
            tx.commit()?;
            Ok(FileId(id))
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
        self.mutate(|this| {
            let id = this.lookup(parent, name)?;
            if this.stat(id)?.kind != Kind::File {
                return Err(Error::IsDirectory);
            }
            let time = Timestamp::now()?;
            let tx = this.db.transaction()?;
            tx.execute(
                "DELETE FROM entries WHERE parent=?1 AND name_key=?2",
                params![parent.0, names::key(name)?],
            )?;
            metadata::touch(&tx, this.catalog_version, parent, time, true, false)?;
            metadata::touch(&tx, this.catalog_version, id, time, false, false)?;
            tx.commit()?;
            Ok(())
        })
    }
    pub fn rmdir(&mut self, parent: FileId, name: &str) -> Result<()> {
        self.mutate(|this| {
            let id = this.lookup(parent, name)?;
            this.directory(id)?;
            if !this.list(id)?.is_empty() {
                return Err(Error::NotEmpty);
            }
            let time = Timestamp::now()?;
            let tx = this.db.transaction()?;
            tx.execute(
                "DELETE FROM entries WHERE parent=?1 AND name_key=?2",
                params![parent.0, names::key(name)?],
            )?;
            tx.execute("DELETE FROM objects WHERE id=?1", [id.0])?;
            metadata::touch(&tx, this.catalog_version, parent, time, true, false)?;
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
        self.mutate(|this| {
            this.directory(from_parent)?;
            this.directory(to_parent)?;
            let old_key = names::key(from)?;
            let new_key = names::key(to)?;
            let source = this.lookup_key(from_parent, &old_key)?.ok_or(Error::NotFound)?;
            let target = this.lookup_key(to_parent, &new_key)?;
            let same_entry = from_parent == to_parent && old_key == new_key;
            if same_entry {
                let time = Timestamp::now()?;
                let tx = this.db.transaction()?;
                tx.execute("UPDATE entries SET display_name=?1 WHERE parent=?2 AND name_key=?3", params![to, from_parent.0, old_key])?;
                metadata::touch(&tx, this.catalog_version, from_parent, time, true, false)?;
                metadata::touch(&tx, this.catalog_version, source, time, false, false)?;
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
            let tx = this.db.transaction()?;
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
            tx.commit()?;
            Ok(())
        })
    }
}
