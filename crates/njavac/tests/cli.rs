use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn cli_writes_the_public_compile_result() {
    let directory = std::env::temp_dir().join(format!(
        "njavac-cli-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),
    ));
    let output = directory.join("out");
    std::fs::create_dir_all(&output).unwrap();
    let source = directory.join("Empty.java");
    let source_text = "public class Empty { public static void main(String[] args) {} }\n";
    std::fs::write(&source, source_text).unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_njavac"))
        .arg("-d")
        .arg(&output)
        .arg(&source)
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        std::fs::read(output.join("Empty.class")).unwrap(),
        njavac::compile(source_text, "Empty.java").unwrap(),
    );

    std::fs::remove_dir_all(directory).unwrap();
}
