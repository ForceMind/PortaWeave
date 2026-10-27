//! Gate the experimental schema before querying user tables or running GC.
//! This is not a complete hostile-input parser or a read-only recovery path.
use crate::{Error, Result};
use rusqlite::Connection;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CatalogVersion {
    BlocksV1,
    MetadataV2,
    DirectoryV3,
}
impl CatalogVersion {
    pub(crate) fn marker(self) -> &'static str {
        match self {
            Self::BlocksV1 => crate::FORMAT,
            Self::MetadataV2 => crate::METADATA_FORMAT,
            Self::DirectoryV3 => crate::DIRECTORY_FORMAT,
        }
    }
    pub(crate) fn from_marker(marker: &[u8]) -> Result<Self> {
        match marker {
            m if m == crate::FORMAT.as_bytes() => Ok(Self::BlocksV1),
            m if m == crate::METADATA_FORMAT.as_bytes() => Ok(Self::MetadataV2),
            m if m == crate::DIRECTORY_FORMAT.as_bytes() => Ok(Self::DirectoryV3),
            _ => Err(Error::UnsupportedFormat),
        }
    }
    pub(crate) fn schema(self) -> &'static str {
        match self {
            Self::BlocksV1 => include_str!("schema.sql"),
            Self::MetadataV2 => include_str!("schema-v2.sql"),
            Self::DirectoryV3 => include_str!("schema-v3.sql"),
        }
    }
    pub(crate) fn has_metadata(self) -> bool {
        self != Self::BlocksV1
    }
    pub(crate) fn capabilities(self) -> u64 {
        match self {
            Self::BlocksV1 => 0,
            Self::MetadataV2 => 1,
            Self::DirectoryV3 => 3,
        }
    }
}

type SchemaEntry = (String, String, String, Option<String>);

fn entries(db: &Connection) -> Result<Vec<SchemaEntry>> {
    let (count, bytes): (i64, i64) = db.query_row(
        "SELECT COUNT(*),COALESCE(SUM(COALESCE(length(CAST(sql AS BLOB)),0)+length(CAST(type AS BLOB))+length(CAST(name AS BLOB))+length(CAST(tbl_name AS BLOB))),0) FROM sqlite_schema",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    // Check before copying SQL strings into Rust. SQLite's own parser and page
    // cache have separate costs and are not bounded by these admission limits.
    if !(0..=32).contains(&count) || !(0..=65_536).contains(&bytes) {
        return Err(Error::UnsupportedFormat);
    }
    let mut statement =
        db.prepare("SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name")?;
    Ok(statement
        .query_map([], |row| {
            let sql: Option<String> = row.get(3)?;
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                sql.map(|text| text.replace("\r\n", "\n")),
            ))
        })?
        .collect::<std::result::Result<_, _>>()?)
}

pub(crate) fn validate_schema(db: &Connection) -> Result<CatalogVersion> {
    let app: i64 = db.query_row("PRAGMA application_id", [], |r| r.get(0))?;
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let version = match (app, version) {
        (1347898693, 1) => CatalogVersion::BlocksV1,
        (1347898693, 2) => CatalogVersion::MetadataV2,
        (1347898693, 3) => CatalogVersion::DirectoryV3,
        _ => return Err(Error::UnsupportedFormat),
    };
    // Derive the accepted representation from the SAME committed schema and
    // bundled SQLite, rather than maintaining a second hand-written signature.
    // No SQL from the input catalog is executed in this temporary connection.
    let expected = Connection::open_in_memory()?;
    expected.execute_batch(version.schema())?;
    if entries(db)? != entries(&expected)? {
        return Err(Error::UnsupportedFormat);
    }
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_gate_accepts_both_source_checkout_line_endings() {
        for version in [
            CatalogVersion::BlocksV1,
            CatalogVersion::MetadataV2,
            CatalogVersion::DirectoryV3,
        ] {
            let unix = version.schema().replace("\r\n", "\n");
            for sql in [unix.clone(), unix.replace('\n', "\r\n")] {
                let db = Connection::open_in_memory().unwrap();
                db.execute_batch(&sql).unwrap();
                assert_eq!(validate_schema(&db).unwrap(), version);
            }
        }
    }
}
