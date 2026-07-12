//! `classdiff` — inspect or structurally diff `.class` files (ROADMAP §0.3).
//!
//!   classdiff <a.class>            # dump one file's structural fields
//!   classdiff <a.class> <b.class>  # diff two files, localizing the first divergence
//!
//! The diff reports the first structurally-divergent field with its byte offset,
//! which localizes to the *cause* even when `javap` output matches (see
//! `njavac::classdump`). Exit status: 0 if identical (or on a plain dump), 1 if the
//! two files differ.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [one] => match read(one) {
            Ok(bytes) => match njavac::classdump::dump(&bytes) {
                Ok(fields) => {
                    print!("{}", njavac::classdump::render_dump(&fields));
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("classdiff: cannot parse {one}: {e}");
                    ExitCode::FAILURE
                }
            },
            Err(e) => {
                eprintln!("classdiff: {e}");
                ExitCode::FAILURE
            }
        },
        [a, b] => {
            let (ba, bb) = match (read(a), read(b)) {
                (Ok(ba), Ok(bb)) => (ba, bb),
                (Err(e), _) | (_, Err(e)) => {
                    eprintln!("classdiff: {e}");
                    return ExitCode::FAILURE;
                }
            };
            match njavac::classdump::diff_report(&ba, &bb) {
                None => {
                    println!("identical ({} bytes)", ba.len());
                    ExitCode::SUCCESS
                }
                Some(report) => {
                    print!("{report}");
                    ExitCode::FAILURE
                }
            }
        }
        _ => {
            eprintln!("usage: classdiff <a.class> [<b.class>]");
            ExitCode::from(2)
        }
    }
}

fn read(path: &str) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("cannot read {path}: {e}"))
}
