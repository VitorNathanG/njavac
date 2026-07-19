use std::collections::{HashMap, HashSet};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use crate::model::Prog;

pub(super) fn reset_dir(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).expect("create dir");
}

/// One `javac -d <out> @<argfile>` invocation. Returns whether it exited zero.
pub(super) fn run_javac_batch(javac: &str, out: &Path, argfile: &Path) -> bool {
    Command::new(javac)
        .arg("-d")
        .arg(out)
        .arg(format!("@{}", argfile.display()))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub(super) fn run_javac_one(javac: &str, out: &Path, src: &Path) -> bool {
    Command::new(javac)
        .arg("-d")
        .arg(out)
        .arg(src)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// The `java` launcher next to a `.../bin/javac` (they share a `bin/`); falls back
/// to a bare `java` on `PATH`. Overridable with the `JAVA` env var.
pub(super) fn derive_java(javac: &str) -> String {
    std::env::var("JAVA").unwrap_or_else(|_| {
        javac
            .strip_suffix("javac")
            .map(|prefix| format!("{prefix}java"))
            .unwrap_or_else(|| "java".to_string())
    })
}

/// A persistent in-memory `javac` worker (`tools/FuzzJavac.java`, source-launched
/// once). The protocol is documented in that unchanged worker source.
pub(super) struct JavacWorker {
    child: Child,
    /// `Option` so `Drop` can `take()` it to close the pipe (EOF -> worker exits)
    /// BEFORE reaping — otherwise `wait()` deadlocks on a worker still reading.
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl JavacWorker {
    pub(super) fn spawn(java: &str, worker_src: &Path) -> JavacWorker {
        let mut child = Command::new(java)
            .arg(worker_src)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|e| {
                panic!("fuzz: cannot spawn javac worker `{java} {}`: {e}", worker_src.display())
            });
        let stdin = child.stdin.take().expect("worker stdin");
        let stdout = BufReader::new(child.stdout.take().expect("worker stdout"));
        JavacWorker { child, stdin: Some(stdin), stdout }
    }

    /// Compile a whole batch in one javac task. Missing expected classes are
    /// rejects; unexpected classes are guarded separately and remain fatal.
    pub(super) fn compile_batch(&mut self, units: &[(&str, &str)]) -> HashMap<String, Vec<u8>> {
        self.request_batch(units)
            .unwrap_or_else(|e| panic!("fuzz: javac worker protocol error ({e}) — worker crashed?"))
    }

    fn request_batch(&mut self, units: &[(&str, &str)]) -> std::io::Result<HashMap<String, Vec<u8>>> {
        let stdin = self.stdin.as_mut().expect("worker stdin already closed");
        stdin.write_all(&(units.len() as u32).to_be_bytes())?;
        for (name, src) in units {
            write_frame(stdin, name.as_bytes())?;
            write_frame(stdin, src.as_bytes())?;
        }
        stdin.flush()?;
        let n = read_i32(&mut self.stdout)?;
        let mut classes = HashMap::with_capacity(n.max(0) as usize);
        for _ in 0..n {
            let name = String::from_utf8(read_frame(&mut self.stdout)?)
                .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "non-utf8 class name"))?;
            let bytes = read_frame(&mut self.stdout)?;
            classes.insert(name, bytes);
        }
        Ok(classes)
    }
}

impl Drop for JavacWorker {
    fn drop(&mut self) {
        // Close stdin first so the worker sees EOF before it is reaped.
        self.stdin.take();
        let _ = self.child.wait();
    }
}

fn write_frame(w: &mut impl Write, bytes: &[u8]) -> std::io::Result<()> {
    w.write_all(&(bytes.len() as u32).to_be_bytes())?;
    w.write_all(bytes)
}

fn read_i32(r: &mut impl Read) -> std::io::Result<i32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(i32::from_be_bytes(b))
}

fn read_frame(r: &mut impl Read) -> std::io::Result<Vec<u8>> {
    let len = read_i32(r)?;
    if len < 0 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "negative frame length"));
    }
    let mut buf = vec![0u8; len as usize];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

/// Every class javac emitted for the batch must belong to an expected unit.
pub(super) fn assert_batch_classes(classes: &HashMap<String, Vec<u8>>, progs: &[Prog]) {
    let expected: HashSet<&str> = progs.iter().map(|p| p.name.class.as_str()).collect();
    for name in classes.keys() {
        assert!(
            expected.contains(name.as_str()),
            "fuzz: javac worker emitted unexpected class {name} — generator over-reached into \
             auxiliary classes (this would compare half a program)"
        );
    }
}

pub(super) fn assert_no_unexpected_classes(javac_out: &Path, progs: &[Prog]) {
    let expected: HashSet<String> = progs.iter().map(|p| format!("{}.class", p.name.class)).collect();
    if let Ok(rd) = std::fs::read_dir(javac_out) {
        for e in rd.flatten() {
            let fname = e.file_name().to_string_lossy().into_owned();
            if fname.ends_with(".class") && !expected.contains(&fname) {
                panic!(
                    "fuzz: unexpected class {fname} in javac output — the generator over-reached \
                     into auxiliary classes (this would compare half a program)"
                );
            }
        }
    }
}

/// The worker's Java source. The sanctioned fuzz image sets `FUZZ_WORKER` to its
/// baked source; the relative path is the direct-binary fallback.
pub(super) fn worker_src_path() -> PathBuf {
    std::env::var("FUZZ_WORKER")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("tools/FuzzJavac.java"))
}
