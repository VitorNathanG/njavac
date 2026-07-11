//! njavac CLI — compile Java sources to `.class` files, javac-style.
//!
//!   njavac [-d <dir>] <file.java> [<file.java> ...]
//!
//! Like javac, a single invocation compiles any number of source files. Each
//! class is written to `<ClassName>.class` — under `-d <dir>` when given,
//! otherwise beside its source file (javac's default). The class name comes from
//! the parsed source; in njavac's supported subset it always matches the
//! basename. As with javac, one source failing does not abort the others; the
//! process exits non-zero if any did.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    // A per-file compiler error is reported below via catch_unwind; silence the
    // default panic dump so the CLI speaks in one voice.
    std::panic::set_hook(Box::new(|_| {}));

    let mut out_dir: Option<PathBuf> = None;
    let mut inputs: Vec<String> = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-d" => out_dir = Some(PathBuf::from(args.next().unwrap_or_else(|| usage("-d needs a directory")))),
            "-h" | "--help" => {
                println!("usage: njavac [-d <dir>] <file.java> [<file.java> ...]");
                return ExitCode::SUCCESS;
            }
            flag if flag.starts_with('-') && flag.len() > 1 => usage(&format!("unknown option: {flag}")),
            _ => inputs.push(a),
        }
    }

    if inputs.is_empty() {
        usage("no source files");
    }
    if let Some(dir) = &out_dir {
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("njavac: cannot create {}: {e}", dir.display());
            return ExitCode::FAILURE;
        }
    }

    let mut failed = false;
    for input in &inputs {
        if let Err(msg) = compile_one(input, out_dir.as_deref()) {
            eprintln!("njavac: {msg}");
            failed = true;
        }
    }
    if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

/// Compile one source file, writing `<ClassName>.class`. Returns a human-readable
/// error — an I/O failure or a compiler panic on unsupported input — so the
/// caller can report it and keep going, the way javac keeps compiling the
/// remaining sources after one fails.
fn compile_one(input: &str, out_dir: Option<&Path>) -> Result<(), String> {
    let path = Path::new(input);
    let source = std::fs::read_to_string(path).map_err(|e| format!("{input}: {e}"))?;
    let source_file = path
        .file_name()
        .ok_or_else(|| format!("{input}: not a file"))?
        .to_string_lossy()
        .into_owned();

    // The output basename is the source basename minus ".java"; in the supported
    // subset the public class name is required to match it, so this is exactly
    // javac's `<ClassName>.class`.
    let class_name = source_file.strip_suffix(".java").unwrap_or(&source_file).to_owned();

    let bytes = std::panic::catch_unwind(|| njavac::compile(&source, &source_file))
        .map_err(|_| format!("{input}: unsupported (compiler error)"))?;

    // javac's rule: with -d write under that directory, otherwise beside the
    // source (a bare filename has an empty parent, i.e. the current directory).
    let dir = out_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| path.parent().map(Path::to_path_buf).unwrap_or_default());
    let dest = dir.join(format!("{class_name}.class"));
    std::fs::write(&dest, bytes).map_err(|e| format!("{}: {e}", dest.display()))
}

fn usage(msg: &str) -> ! {
    eprintln!("njavac: {msg}");
    eprintln!("usage: njavac [-d <dir>] <file.java> [<file.java> ...]");
    std::process::exit(2);
}
