use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rayon::prelude::*;

mod format;
use format::{Codec, JpegToWebp};

const BINARY_NAME: &str = "gallery-compress";
const DEFAULT_GALLERY_BASE: &str =
    "/home/alex/Er/VFSite/vfatina-home/public/images/gallery";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        print_usage();
        return ExitCode::from(2);
    }
    let target = &args[1];

    let gallery_base =
        env::var("GALLERY_BASE").unwrap_or_else(|_| DEFAULT_GALLERY_BASE.to_string());

    let dir: PathBuf = if target.contains('/') {
        PathBuf::from(target)
    } else {
        PathBuf::from(&gallery_base).join(target)
    };

    if !dir.is_dir() {
        eprintln!("{BINARY_NAME}: not a directory: {}", dir.display());
        return ExitCode::from(1);
    }

    let codec = JpegToWebp;
    let jpgs: Vec<PathBuf> = match fs::read_dir(&dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| has_accepted_extension(p, codec.accepted_extensions()))
            .collect(),
        Err(e) => {
            eprintln!("{BINARY_NAME}: cannot read {}: {}", dir.display(), e);
            return ExitCode::from(1);
        }
    };

    if jpgs.is_empty() {
        println!("{BINARY_NAME}: no .jpg files in {}", dir.display());
        return ExitCode::from(0);
    }

    let n = jpgs.len();
    let results: Vec<Result<(u64, u64), String>> = jpgs
        .par_iter()
        .map(|src| {
            let dst = src.with_extension(codec.output_extension());
            let report = codec
                .convert_one(src, &dst)
                .map_err(|e| format!("{}: {}", src.display(), e))?;
            fs::remove_file(src).map_err(|e| e.to_string())?;
            Ok((report.in_bytes, report.out_bytes))
        })
        .collect();

    let mut count: u64 = 0;
    let mut total_in: u64 = 0;
    let mut total_out: u64 = 0;
    let mut failed: u64 = 0;
    for r in results {
        match r {
            Ok((i, o)) => {
                count += 1;
                total_in += i;
                total_out += o;
            }
            Err(msg) => {
                eprintln!("{BINARY_NAME}: {}", msg);
                failed += 1;
            }
        }
    }

    println!(
        "{BINARY_NAME}: {} files in {}: {} -> {}",
        count,
        dir.display(),
        human_bytes(total_in),
        human_bytes(total_out)
    );
    eprintln!("(processed {} candidates, {} failed)", n, failed);

    if failed > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::from(0)
    }
}

fn has_accepted_extension(p: &Path, accepted: &'static [&'static str]) -> bool {
    let ext = match p.extension().and_then(|x| x.to_str()) {
        Some(e) => e.to_ascii_lowercase(),
        None => return false,
    };
    accepted.iter().any(|a| a.eq_ignore_ascii_case(&ext))
}

fn print_usage() {
    eprintln!(
        "Usage: gallery-compress <year|dir>\n\
         \n\
         Examples:\n\
         \x20 gallery-compress 2025\n\
         \x20 gallery-compress /tmp/my-images\n\
         \n\
         Env:\n\
         \x20 GALLERY_BASE  default: {DEFAULT_GALLERY_BASE}"
    );
}

fn human_bytes(n: u64) -> String {
    const UNITS: &[&str] = &["B", "K", "M", "G", "T"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{}{}", n, UNITS[0])
    } else {
        format!("{:.1}{}", v, UNITS[i])
    }
}
