use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rayon::prelude::*;

mod format;
mod params;
use format::{
    CodecImpl, Format, JpegToPng, JpegToWebp, PngToJpeg, PngToWebp, WebpToJpeg,
    WebpToPng,
};

const BINARY_NAME: &str = "convert-to-webp";
const DEFAULT_GALLERY_BASE: &str =
    "/home/alex/Er/VFSite/vfatina-home/public/images/gallery";

#[derive(Debug, Clone, PartialEq, Eq)]
enum CliError {
    Usage,
    BadInputFormat(String),
    BadOutputFormat(String),
}

#[derive(Debug)]
struct Cli {
    positional: String,
    input_format: Option<Format>,
    output_format: Option<Format>,
}

fn parse_cli(args: &[String]) -> Result<Cli, CliError> {
    let mut positional: Option<String> = None;
    let mut input_format: Option<Format> = None;
    let mut output_format: Option<Format> = None;

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--input-format" => {
                let v = args.get(i + 1).ok_or(CliError::Usage)?;
                input_format = Some(Format::parse(v).ok_or_else(|| {
                    CliError::BadInputFormat((*v).clone())
                })?);
                i += 2;
            }
            "--output-format" => {
                let v = args.get(i + 1).ok_or(CliError::Usage)?;
                output_format = Some(Format::parse(v).ok_or_else(|| {
                    CliError::BadOutputFormat((*v).clone())
                })?);
                i += 2;
            }
            "-h" | "--help" => return Err(CliError::Usage),
            other if other.starts_with("--") => {
                eprintln!(
                    "{BINARY_NAME}: unknown flag: {other}\n\
                     Try '{BINARY_NAME} --help' for usage."
                );
                return Err(CliError::Usage);
            }
            _ => {
                if positional.is_some() {
                    eprintln!("{BINARY_NAME}: unexpected extra arg: {arg}");
                    return Err(CliError::Usage);
                }
                positional = Some(arg.clone());
                i += 1;
            }
        }
    }

    Ok(Cli {
        positional: positional.ok_or(CliError::Usage)?,
        input_format,
        output_format,
    })
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let cli = match parse_cli(&args) {
        Ok(c) => c,
        Err(e) => {
            match e {
                CliError::BadInputFormat(v) => eprintln!(
                    "{BINARY_NAME}: invalid --input-format: {v} (expected jpg, png, or webp)"
                ),
                CliError::BadOutputFormat(v) => eprintln!(
                    "{BINARY_NAME}: invalid --output-format: {v} (expected jpg, png, or webp)"
                ),
                CliError::Usage => {}
            }
            print_usage();
            return ExitCode::from(2);
        }
    };

    let target = cli.positional;
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

    // Default = v0 behaviour: jpg -> webp. Both flags are explicit
    // overrides; the absent pair remains the v0 default.
    let input_format = cli.input_format.unwrap_or(Format::Jpg);
    let output_format = cli.output_format.unwrap_or(Format::Webp);

    let codec: CodecImpl = match (input_format, output_format) {
        (Format::Jpg, Format::Webp) => CodecImpl::JpegToWebp(JpegToWebp),
        (Format::Png, Format::Webp) => CodecImpl::PngToWebp(PngToWebp),
        (Format::Webp, Format::Png) => CodecImpl::WebpToPng(WebpToPng),
        (Format::Webp, Format::Jpg) => CodecImpl::WebpToJpeg(WebpToJpeg),
        (Format::Jpg, Format::Png) => CodecImpl::JpegToPng(JpegToPng),
        (Format::Png, Format::Jpg) => CodecImpl::PngToJpeg(PngToJpeg),
        (Format::Jpg, Format::Jpg)
        | (Format::Png, Format::Png)
        | (Format::Webp, Format::Webp) => {
            eprintln!(
                "{BINARY_NAME}: same input/output format ({input_format:?}) \
                 is a no-op; refusing to overwrite the source."
            );
            return ExitCode::from(2);
        }
    };

    let candidates: Vec<PathBuf> = match fs::read_dir(&dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && has_accepted_extension(p, codec.accepted_extensions())
            })
            .collect(),
        Err(e) => {
            eprintln!("{BINARY_NAME}: cannot read {}: {}", dir.display(), e);
            return ExitCode::from(1);
        }
    };

    if candidates.is_empty() {
        let accepted = codec.accepted_extensions().join(", .");
        println!(
            "{BINARY_NAME}: no candidate files (.{accepted}) in {}",
            dir.display()
        );
        return ExitCode::from(0);
    }

    let n = candidates.len();
    let results: Vec<Result<(u64, u64), String>> = candidates
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
        "Usage: convert-to-webp <dir> [--input-format <fmt>] [--output-format <fmt>]\n\
         \n\
         Arguments:\n\
         \x20 <dir>                  directory containing the input images\n\
         \n\
         Flags:\n\
         \x20 --input-format <fmt>   one of: jpg, png, webp (default: jpg)\n\
         \x20 --output-format <fmt>  one of: jpg, png, webp (default: webp)\n\
         \x20 -h, --help             show this help\n\
         \n\
         Examples:\n\
         \x20 convert-to-webp /tmp/my-images\n\
         \x20 convert-to-webp /tmp/my-images --input-format png --output-format webp\n\
         \x20 convert-to-webp /tmp/my-images --input-format webp --output-format png\n\
         \x20 convert-to-webp /tmp/my-images --input-format webp --output-format jpg\n\
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

#[cfg(test)]
mod cli_tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn parses_default_invocation() {
        let cli = parse_cli(&v(&["gallery-compress", "/tmp/x"])).unwrap();
        assert_eq!(cli.positional, "/tmp/x");
        assert_eq!(cli.input_format, None);
        assert_eq!(cli.output_format, None);
    }

    #[test]
    fn parses_explicit_flags() {
        let cli = parse_cli(&v(&[
            "gallery-compress",
            "/tmp/x",
            "--input-format",
            "png",
            "--output-format",
            "webp",
        ]))
        .unwrap();
        assert_eq!(cli.input_format, Some(Format::Png));
        assert_eq!(cli.output_format, Some(Format::Webp));
    }

    #[test]
    fn rejects_bad_input_format() {
        let err = parse_cli(&v(&[
            "gallery-compress",
            "/tmp/x",
            "--input-format",
            "tiff",
        ]))
        .unwrap_err();
        assert!(matches!(err, CliError::BadInputFormat(s) if s == "tiff"));
    }

    #[test]
    fn rejects_bad_output_format() {
        let err = parse_cli(&v(&[
            "gallery-compress",
            "/tmp/x",
            "--output-format",
            "gif",
        ]))
        .unwrap_err();
        assert!(matches!(err, CliError::BadOutputFormat(s) if s == "gif"));
    }

    #[test]
    fn rejects_unknown_flag() {
        let err = parse_cli(&v(&["gallery-compress", "--foo"])).unwrap_err();
        assert_eq!(err, CliError::Usage);
    }

    #[test]
    fn rejects_help() {
        let err = parse_cli(&v(&["gallery-compress", "--help"])).unwrap_err();
        assert_eq!(err, CliError::Usage);
    }

    #[test]
    fn rejects_missing_positional() {
        let err = parse_cli(&v(&["gallery-compress"])).unwrap_err();
        assert_eq!(err, CliError::Usage);
    }

    #[test]
    fn rejects_extra_positional() {
        let err = parse_cli(&v(&["gallery-compress", "/tmp/a", "/tmp/b"])).unwrap_err();
        assert_eq!(err, CliError::Usage);
    }

    #[test]
    fn format_parse_accepts_three_values() {
        assert_eq!(Format::parse("jpg"), Some(Format::Jpg));
        assert_eq!(Format::parse("jpeg"), Some(Format::Jpg));
        assert_eq!(Format::parse("JPEG"), Some(Format::Jpg));
        assert_eq!(Format::parse("png"), Some(Format::Png));
        assert_eq!(Format::parse("PNG"), Some(Format::Png));
        assert_eq!(Format::parse("webp"), Some(Format::Webp));
        assert_eq!(Format::parse("WebP"), Some(Format::Webp));
        assert_eq!(Format::parse("tiff"), None);
    }
}
