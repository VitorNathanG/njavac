use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

const READY: i32 = 0x4e4a4f42;
const MAX_FRAME_LEN: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Termination {
    Returned,
    Threw,
    LoadFailed,
    TimedOut,
    NotRun,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Observation {
    pub(super) termination: Termination,
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: Vec<u8>,
    pub(super) detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ObservationPair {
    pub(super) reference: Observation,
    pub(super) candidate: Observation,
}

pub(super) struct ObserveWorker {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    java: String,
    worker_src: PathBuf,
}

impl ObserveWorker {
    pub(super) fn spawn(java: &str, worker_src: &Path) -> ObserveWorker {
        let (child, stdin, stdout) = spawn_process(java, worker_src);
        ObserveWorker {
            child,
            stdin: Some(stdin),
            stdout,
            java: java.to_string(),
            worker_src: worker_src.to_path_buf(),
        }
    }

    pub(super) fn observe_pair(
        &mut self,
        name: &str,
        reference: &[u8],
        candidate: &[u8],
    ) -> ObservationPair {
        let pair = self.request_pair(name, reference, candidate).unwrap_or_else(|e| {
            panic!("fuzz: observer worker protocol error ({e}) - worker crashed?")
        });
        if pair.reference.termination == Termination::TimedOut {
            self.restart();
            let reversed = self.request_pair(name, candidate, reference).unwrap_or_else(|e| {
                panic!("fuzz: observer worker protocol error ({e}) - worker crashed?")
            });
            if reversed.reference.termination == Termination::TimedOut
                || reversed.candidate.termination == Termination::TimedOut
            {
                self.restart();
            }
            return ObservationPair {
                reference: pair.reference,
                candidate: reversed.reference,
            };
        }
        if pair.candidate.termination == Termination::TimedOut {
            self.restart();
        }
        pair
    }

    fn request_pair(
        &mut self,
        name: &str,
        reference: &[u8],
        candidate: &[u8],
    ) -> std::io::Result<ObservationPair> {
        let stdin = self.stdin.as_mut().expect("observer stdin already closed");
        write_frame(stdin, name.as_bytes())?;
        write_frame(stdin, reference)?;
        write_frame(stdin, candidate)?;
        stdin.flush()?;

        let pair = ObservationPair {
            reference: read_observation(&mut self.stdout)?,
            candidate: read_observation(&mut self.stdout)?,
        };
        validate_pair(&pair)?;
        Ok(pair)
    }

    fn restart(&mut self) {
        self.stdin.take();
        let _ = self.child.wait();
        let (child, stdin, stdout) = spawn_process(&self.java, &self.worker_src);
        self.child = child;
        self.stdin = Some(stdin);
        self.stdout = stdout;
    }
}

impl Drop for ObserveWorker {
    fn drop(&mut self) {
        self.stdin.take();
        let _ = self.child.wait();
    }
}

fn spawn_process(java: &str, worker_src: &Path) -> (Child, ChildStdin, BufReader<ChildStdout>) {
    let mut child = Command::new(java)
        .arg("-Xverify:all")
        .arg(worker_src)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| {
            panic!(
                "fuzz: cannot spawn observer worker `{java} -Xverify:all {}`: {e}",
                worker_src.display()
            )
        });
    let stdin = child.stdin.take().expect("observer stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("observer stdout"));
    let ready = read_i32(&mut stdout).unwrap_or_else(|e| {
        let _ = child.kill();
        let _ = child.wait();
        panic!("fuzz: observer worker failed before READY ({e})")
    });
    if ready != READY {
        let _ = child.kill();
        let _ = child.wait();
        panic!("fuzz: observer worker sent invalid READY marker {ready:#x}");
    }
    (child, stdin, stdout)
}

fn write_frame(w: &mut impl Write, bytes: &[u8]) -> std::io::Result<()> {
    let len = i32::try_from(bytes.len())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "frame too large"))?;
    w.write_all(&len.to_be_bytes())?;
    w.write_all(bytes)
}

fn read_i32(r: &mut impl Read) -> std::io::Result<i32> {
    let mut bytes = [0u8; 4];
    r.read_exact(&mut bytes)?;
    Ok(i32::from_be_bytes(bytes))
}

fn read_frame(r: &mut impl Read) -> std::io::Result<Vec<u8>> {
    let len = read_i32(r)?;
    if len < 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "negative frame length",
        ));
    }
    if len as usize > MAX_FRAME_LEN {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "observer frame too large",
        ));
    }
    let mut bytes = vec![0u8; len as usize];
    r.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn read_observation(r: &mut impl Read) -> std::io::Result<Observation> {
    let status = read_i32(r)?;
    let stdout = read_frame(r)?;
    let stderr = read_frame(r)?;
    let detail = String::from_utf8(read_frame(r)?)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "non-utf8 detail"))?;
    let termination = match status {
        0 if detail.is_empty() => Termination::Returned,
        1 if !detail.is_empty() => Termination::Threw,
        2 if !detail.is_empty() => Termination::LoadFailed,
        3 if detail.is_empty() => Termination::TimedOut,
        4 if detail.is_empty() => Termination::NotRun,
        0..=4 => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid detail for observer status",
            ));
        }
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "unknown observer status",
            ));
        }
    };
    Ok(Observation { termination, stdout, stderr, detail })
}

fn validate_pair(pair: &ObservationPair) -> std::io::Result<()> {
    let valid = match pair.reference.termination {
        Termination::NotRun => false,
        Termination::TimedOut => pair.candidate.termination == Termination::NotRun,
        _ => pair.candidate.termination != Termination::NotRun,
    };
    if !valid || (pair.candidate.termination == Termination::NotRun
        && (!pair.candidate.stdout.is_empty() || !pair.candidate.stderr.is_empty()))
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid observer pair statuses",
        ));
    }
    Ok(())
}

pub(super) fn observer_src_path() -> PathBuf {
    std::env::var("FUZZ_OBSERVER")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("tools/FuzzObserve.java"))
}
