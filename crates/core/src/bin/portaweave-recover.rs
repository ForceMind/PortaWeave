//! Experimental offline recovery utility. Never mounts a volume or repairs originals.
use portaweave_core::{ExportLinks, Recovery, RecoveryLimits};
use std::{ffi::OsString, path::PathBuf};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let verify = args.first().is_some_and(|a| a == "verify") && args.len() == 3;
    let export = args.first().is_some_and(|a| a == "export")
        && (args.len() == 4 || (args.len() == 5 && args[4] == "--copy-hardlinks"));
    if !verify && !export {
        return Err("usage: portaweave-recover verify SOURCE NEW_WORKSPACE\n       portaweave-recover export SOURCE NEW_WORKSPACE NEW_OUTPUT [--copy-hardlinks]\nExperimental: copies evidence first; no repair of originals, no overwrite. Default limits: 64 GiB snapshot/export, 1M catalog rows, depth 256. Failed workspaces are retained.".into());
    }
    let recovery = Recovery::prepare(PathBuf::from(&args[1]), PathBuf::from(&args[2]), RecoveryLimits::default())?;
    println!("Evidence and isolated working catalog: {:?}", recovery.workspace());
    println!("Verification: {:#?}", recovery.verify()?);
    if export {
        let mode = if args.len() == 5 { ExportLinks::CopyAliases } else { ExportLinks::Preserve };
        println!("Export: {:#?}", recovery.export_new(PathBuf::from(&args[3]), mode)?);
    }
    println!("Original container was not opened for SQLite recovery or garbage collection.\nContent export is not metadata-complete or a guarantee of physical durability.");
    Ok(())
}
fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Recovery stopped: {error}\nDo not retry into an existing workspace/output. Preserve incomplete evidence for diagnosis.");
            std::process::ExitCode::FAILURE
        }
    }
}
