//! njavac CLI: compile one `.java` file to a `.class` file.
//!
//!   njavac <input.java> <output.class>
//!
//! The class name comes from the source; <output.class> is just where bytes
//! are written (name it to match the class if you intend to run it).

use std::path::Path;

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let (input, output) = match (args.next(), args.next()) {
        (Some(i), Some(o)) => (i, o),
        _ => {
            eprintln!("usage: njavac <input.java> <output.class>");
            std::process::exit(2);
        }
    };
    let source = std::fs::read_to_string(&input)?;
    let source_file = Path::new(&input)
        .file_name()
        .expect("input path has a file name")
        .to_string_lossy()
        .into_owned();
    let bytes = njavac::compile(&source, &source_file);
    std::fs::write(&output, bytes)?;
    Ok(())
}
