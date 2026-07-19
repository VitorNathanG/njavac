//! njavac CLI — compile Java sources to `.class` files, javac-style.
//!
//!   njavac [-d <dir>] <file.java> [<file.java> ...]
//!
//! One invocation independently compiles any number of source files. Output uses
//! the source basename under `-d <dir>` or beside the source; supported inputs
//! require that basename to match the parsed public class. A returned diagnostic
//! for one source does not abort later sources, and any failure makes the process
//! exit non-zero.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut out_dir: Option<PathBuf> = None;
    let mut inputs: Vec<String> = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-d" => {
                out_dir = Some(PathBuf::from(
                    args.next().unwrap_or_else(|| usage("-d needs a directory")),
                ))
            }
            "-h" | "--help" => {
                println!("usage: njavac [-d <dir>] <file.java> [<file.java> ...]");
                return ExitCode::SUCCESS;
            }
            flag if flag.starts_with('-') && flag.len() > 1 => {
                usage(&format!("unknown option: {flag}"))
            }
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
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Compile one source file, writing `<ClassName>.class`. Returns a human-readable
/// error from I/O or a returned compile diagnostic, so the caller can report it
/// and keep going. Internal invariant failures remain ordinary Rust panics.
fn compile_one(input: &str, out_dir: Option<&Path>) -> Result<(), String> {
    let path = Path::new(input);
    let source = std::fs::read_to_string(path).map_err(|e| format!("{input}: {e}"))?;
    let source_file = path
        .file_name()
        .ok_or_else(|| format!("{input}: not a file"))?
        .to_string_lossy()
        .into_owned();

    // The CLI names output from the source basename. The supported-language
    // contract requires it to match the public class, but compile() does not
    // currently enforce that relationship.
    let class_name = source_file
        .strip_suffix(".java")
        .unwrap_or(&source_file)
        .to_owned();

    let bytes = njavac::compile(&source, &source_file)
        .map_err(|diagnostic| diagnostic.render(input, &source))?;

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
