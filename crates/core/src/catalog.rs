//! Gate the experimental schema before querying user tables or running GC.
//! This is not a complete hostile-input parser or a read-only recovery path.
use crate::{Error, Result};
use rusqlite::Connection;

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

pub(crate) fn validate_schema(db: &Connection) -> Result<()> {
    // Derive the accepted representation from the SAME committed schema and
    // bundled SQLite, rather than maintaining a second hand-written signature.
    // No SQL from the input catalog is executed in this temporary connection.
    let expected = Connection::open_in_memory()?;
    expected.execute_batch(include_str!("schema.sql"))?;
    if entries(db)? != entries(&expected)? {
        return Err(Error::UnsupportedFormat);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_gate_accepts_both_source_checkout_line_endings() {
        let unix = include_str!("schema.sql").replace("\r\n", "\n");
        for sql in [unix.clone(), unix.replace('\n', "\r\n")] {
            let db = Connection::open_in_memory().unwrap();
            db.execute_batch(&sql).unwrap();
            validate_schema(&db).unwrap();
        }
    }
}
