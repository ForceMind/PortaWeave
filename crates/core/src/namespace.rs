use crate::{Entry, Error, FileId, Kind, ROOT, Result, Volume, names};
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
        self.observe((|| {
            self.directory(parent)?;
            self.lookup_key(parent, &names::key(name)?)?
                .ok_or(Error::NotFound)
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
    fn create_node(&mut self, parent: FileId, name: &str, kind: i64) -> Result<FileId> {
        self.mutate(|this| {
            this.directory(parent)?;
            let key = names::key(name)?;
            if this.lookup_key(parent, &key)?.is_some() {
                return Err(Error::Exists);
            }
            let tx = this.db.transaction()?;
            tx.execute("INSERT INTO objects(kind) VALUES (?1)", [kind])?;
            let id = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO entries VALUES (?1,?2,?3,?4)",
                params![parent.0, key, name, id],
            )?;
            tx.commit()?;
            Ok(FileId(id))
        })
    }
    pub fn create_file(&mut self, parent: FileId, name: &str) -> Result<FileId> {
        self.create_node(parent, name, 0)
    }
    pub fn mkdir(&mut self, parent: FileId, name: &str) -> Result<FileId> {
        self.create_node(parent, name, 1)
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
            let tx = this.db.transaction()?;
            tx.execute(
                "INSERT INTO entries VALUES (?1,?2,?3,?4)",
                params![parent.0, key, name, object.0],
            )?;
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
            this.db.execute(
                "DELETE FROM entries WHERE parent=?1 AND name_key=?2",
                params![parent.0, names::key(name)?],
            )?;
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
            let tx = this.db.transaction()?;
            tx.execute(
                "DELETE FROM entries WHERE parent=?1 AND name_key=?2",
                params![parent.0, names::key(name)?],
            )?;
            tx.execute("DELETE FROM objects WHERE id=?1", [id.0])?;
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
                this.db.execute("UPDATE entries SET display_name=?1 WHERE parent=?2 AND name_key=?3", params![to, from_parent.0, old_key])?;
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
            let tx = this.db.transaction()?;
            if let Some(id) = target {
                tx.execute("DELETE FROM entries WHERE parent=?1 AND name_key=?2", params![to_parent.0, new_key])?;
                if source_kind == Kind::Directory { tx.execute("DELETE FROM objects WHERE id=?1", [id.0])?; }
            }
            let changed = tx.execute("UPDATE entries SET parent=?1,name_key=?2,display_name=?3 WHERE parent=?4 AND name_key=?5",
                params![to_parent.0, new_key, to, from_parent.0, old_key])?;
            if changed != 1 { return Err(Error::Corrupt("lost source during rename".into())); }
            tx.commit()?;
            Ok(())
        })
    }
}
