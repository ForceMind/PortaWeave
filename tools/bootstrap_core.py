"""One-shot, reviewed bootstrap fixes. Removed before merging this development PR."""
from pathlib import Path


def replace(path: str, old: str, new: str, expected: int = 1) -> None:
    p = Path(path)
    text = p.read_text(encoding="utf-8")
    if text.count(old) != expected:
        raise RuntimeError(f"Unexpected source at {path}: {old[:80]!r}")
    p.write_text(text.replace(old, new), encoding="utf-8")

v = "crates/core/src/volume.rs"
s = "crates/core/src/segment.rs"
t = "crates/core/tests/storage.rs"
replace(s, "pub offset: u64,", "pub offset: i64,")
replace(s, "BlockRef { index: *index, segment: id, offset,", "BlockRef { index: *index, segment: id, offset: i64::try_from(offset).map_err(|_| Error::InvalidRange)?,")
replace(s, "let offset = reference.offset;", 'let offset = u64::try_from(reference.offset).map_err(|_| Error::Corrupt("negative block offset".into()))?;')
replace(s, "(offset - HEADER) % BLOCK_SIZE as u64 != 0", "!(offset - HEADER).is_multiple_of(BLOCK_SIZE as u64)")
replace(v, "    TOKENS.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))\n        .map_err(|_| Error::InvalidRange)", "    let mut current = TOKENS.load(Ordering::Relaxed);\n    loop {\n        let next = current.checked_add(1).ok_or(Error::InvalidRange)?;\n        match TOKENS.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {\n            Ok(value) => return Ok(value), Err(observed) => current = observed,\n        }\n    }")
replace(v, "params![bytes, id]", "params![i64::try_from(bytes).map_err(|_| Error::InvalidRange)?, id]")
replace(v, "params![new, id.0]", "params![new as i64, id.0]")
replace(v, "params![id.0, size.div_ceil(BLOCK_SIZE as u64)]", "params![id.0, size.div_ceil(BLOCK_SIZE as u64) as i64]")
replace(v, "params![size, id.0]", "params![size as i64, id.0]")
replace(v, "size % BLOCK_SIZE as u64 != 0", "!size.is_multiple_of(BLOCK_SIZE as u64)")
replace(v, 'let logical_bytes: u64 = self.db.query_row("SELECT COALESCE(SUM(size),0) FROM objects", [], |r| r.get(0))?;', 'let logical_raw: i64 = self.db.query_row("SELECT COALESCE(SUM(size),0) FROM objects", [], |r| r.get(0))?;\n            let logical_bytes = u64::try_from(logical_raw).map_err(|_| Error::Corrupt("negative logical usage".into()))?;')
replace(v, "    fault: Option<FaultPoint>,", "    fault: Option<FaultPoint>,\n    #[cfg(feature = \"fault-injection\")]\n    exit_on_fault: bool,")
replace(v, "#[cfg(feature = \"fault-injection\")] fault: None, _owner: owner", "#[cfg(feature = \"fault-injection\")] fault: None,\n            #[cfg(feature = \"fault-injection\")] exit_on_fault: false, _owner: owner")
replace(v, "    pub fn inject_once(&mut self, point: FaultPoint) { self.fault = Some(point); }\n    fn take_fault(&mut self, point: u8) -> bool {\n        #[cfg(feature = \"fault-injection\")]\n        if self.fault.is_some_and(|f| f as u8 == point) { self.fault = None; return true; }\n        let _ = point;\n        false\n    }\n    fn hit(&mut self, point: u8) -> Result<()> {\n        if self.take_fault(point) { Err(Error::Injected) } else { Ok(()) }\n    }", "    pub fn inject_once(&mut self, point: FaultPoint) { self.fault = Some(point); self.exit_on_fault = false; }\n    /// Test-only process termination, including INSIDE an active SQLite transaction.\n    /// Never enable the fault-injection feature for a user-facing application.\n    #[cfg(feature = \"fault-injection\")]\n    pub fn inject_process_exit_once(&mut self, point: FaultPoint) { self.fault = Some(point); self.exit_on_fault = true; }\n    fn take_fault(&mut self, point: u8) -> Option<bool> {\n        #[cfg(feature = \"fault-injection\")]\n        if self.fault.is_some_and(|f| f as u8 == point) { self.fault = None; return Some(self.exit_on_fault); }\n        let _ = point;\n        None\n    }\n    fn interrupt(action: Option<bool>) -> Result<()> {\n        match action { Some(true) => std::process::exit(86), Some(false) => Err(Error::Injected), None => Ok(()) }\n    }\n    fn hit(&mut self, point: u8) -> Result<()> {\n        Self::interrupt(self.take_fault(point))\n    }")
replace(v, "if before { return Err(Error::Injected); }", "Self::interrupt(before)?;", 3)
replace(v, '            let ids: Vec<i64> = self.db.prepare("SELECT id FROM objects")?', '            let names: Vec<(String, String)> = self.db.prepare("SELECT name_key,display_name FROM entries")?\n                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<std::result::Result<_, _>>()?;\n            for (key, display) in names {\n                if crate::names::key(&display).ok().as_ref() != Some(&key) { return Err(Error::Corrupt("invalid name comparison key".into())); }\n            }\n            let ids: Vec<i64> = self.db.prepare("SELECT id FROM objects")?')
text = Path(t).read_text(encoding="utf-8")
start = text.index("fn contents(")
stop = text.index("\n#[test]", start)
Path(t).write_text(text[:start] + text[stop:], encoding="utf-8")
replace(t, "        v.inject_once(point);\n        assert!(matches!(v.write(h, 0, b\"NEW-BYTES\"), Err(Error::Injected)));\n        // Exit without running Rust destructors or SQLite close/checkpoint.\n        std::process::exit(86);", "        v.inject_process_exit_once(point);\n        let _ = v.write(h, 0, b\"NEW-BYTES\");\n        panic!(\"fault did not terminate the child process\");")
print("Applied checked source corrections; cargo fmt and lock generation follow.")
