//! Read-only partition-table inspection: no physical devices or changes.
use portaweave_partition_inspector::{inspect_image, Integrity, Scheme};
use std::{env, path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let mut args=env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("inspect")) {
        eprintln!("usage: pw-partitions inspect <RAW/IMG-file> [--sector-size 512|4096]");
        return ExitCode::from(2);
    }
    let Some(path)=args.next() else {
        eprintln!("missing image filename");
        return ExitCode::from(2);
    };
    let mut sector=512;
    if let Some(flag)=args.next() {
        if flag!="--sector-size" { eprintln!("unexpected option"); return ExitCode::from(2); }
        let Some(value)=args.next() else { eprintln!("missing sector size"); return ExitCode::from(2); };
        if value=="4096" { sector=4096; }
        else if value!="512" { eprintln!("sector size must be 512 or 4096"); return ExitCode::from(2); }
    }
    if args.next().is_some() { eprintln!("too many arguments"); return ExitCode::from(2); }
    match inspect_image(PathBuf::from(path),sector) {
        Ok(report)=>{
            println!("{} {} image={}B sector={}B",
                match report.scheme { Scheme::Gpt=>"GPT",Scheme::Mbr=>"MBR" },
                match report.integrity { Integrity::Complete=>"VALIDATED",Integrity::Degraded=>"DEGRADED" },
                report.disk_bytes,report.sector_size);
            for p in report.partitions {
                println!("#{} LBA {}..={} type={} id={} name={} boot={}",
                    p.index,p.first_lba,p.last_lba,p.type_id,
                    p.unique_id.unwrap_or_else(||"-".to_string()),
                    p.name.unwrap_or_else(||"-".to_string()),p.bootable);
            }
            for w in report.warnings { eprintln!("WARNING: {w}"); }
            println!("READ-ONLY: source image unchanged.");
            ExitCode::SUCCESS
        }
        Err(e)=>{ eprintln!("inspection failed: {e}"); ExitCode::from(1) }
    }
}
