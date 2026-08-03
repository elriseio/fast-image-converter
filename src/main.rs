use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use image::imageops::FilterType;
use image::ImageReader;
use rayon::prelude::*;
use webp::Encoder;

mod format;

const QUALITY: f32 = 85.0;
const PORTRAIT_MAX_W: u32 = 800;
const LANDSCAPE_MAX_W: u32 = 1000;
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
        eprintln!("gallery-compress: not a directory: {}", dir.display());
        return ExitCode::from(1);
    }

    let jpgs: Vec<PathBuf> = match fs::read_dir(&dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .and_then(|x| x.to_str())
                    .map(|s| s.eq_ignore_ascii_case("jpg"))
                    .unwrap_or(false)
            })
            .collect(),
        Err(e) => {
            eprintln!("gallery-compress: cannot read {}: {}", dir.display(), e);
            return ExitCode::from(1);
        }
    };

    if jpgs.is_empty() {
        println!("gallery-compress: no .jpg files in {}", dir.display());
        return ExitCode::from(0);
    }

    let n = jpgs.len();
    let results: Vec<Result<(u64, u64), String>> = jpgs
        .par_iter()
        .map(|src| convert_one(src).map_err(|e| format!("{}: {}", src.display(), e)))
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
                eprintln!("gallery-compress: {}", msg);
                failed += 1;
            }
        }
    }

    println!(
        "gallery-compress: {} files in {}: {} -> {}",
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

fn convert_one(src: &Path) -> Result<(u64, u64), String> {
    let in_size = fs::metadata(src).map_err(|e| e.to_string())?.len();

    let img = ImageReader::open(src)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?;

    let (w, h) = (img.width(), img.height());
    let target_w = if h >= w {
        PORTRAIT_MAX_W
    } else {
        LANDSCAPE_MAX_W
    };

    let resized = if w > target_w {
        img.resize(target_w, u32::MAX, FilterType::Lanczos3)
    } else {
        img
    };

    let rgb = resized.to_rgb8();

    let encoder = Encoder::from_rgb(rgb.as_raw(), rgb.width(), rgb.height());
    let memory = encoder.encode(QUALITY);
    let webp_bytes: Vec<u8> = memory.as_ref().to_vec();

    let dst = src.with_extension("webp");
    fs::write(&dst, &webp_bytes).map_err(|e| e.to_string())?;

    fs::remove_file(src).map_err(|e| e.to_string())?;

    Ok((in_size, webp_bytes.len() as u64))
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