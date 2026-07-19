use std::any::Any;

use njavac::diagnostic::{CompileResult, Diagnostic, ErrorKind};
use njavac::span::Span;

/// The complete result of invoking njavac on one generated source.
pub(super) enum NjavacOutcome {
    Accepted(Vec<u8>),
    Unsupported(Diagnostic),
    SyntaxError(Diagnostic),
    InternalPanic(String),
}

impl NjavacOutcome {
    /// Consumers such as the minimizer only operate on accepted class bytes.
    pub(super) fn accepted_bytes(self) -> Option<Vec<u8>> {
        match self {
            Self::Accepted(bytes) => Some(bytes),
            Self::Unsupported(_) | Self::SyntaxError(_) | Self::InternalPanic(_) => None,
        }
    }
}

/// Compile `src` in-process and preserve every candidate outcome. The
/// `source_arg` must match the class and filename token.
pub(super) fn njavac_compile(src: &str, source_arg: &str) -> NjavacOutcome {
    capture_outcome(|| njavac::compile(src, source_arg))
}

fn capture_outcome<F>(compile: F) -> NjavacOutcome
where
    F: FnOnce() -> CompileResult<Vec<u8>>,
{
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(compile)) {
        Ok(Ok(bytes)) => NjavacOutcome::Accepted(bytes),
        Ok(Err(diagnostic)) => match diagnostic.kind() {
            ErrorKind::Unsupported => NjavacOutcome::Unsupported(diagnostic),
            ErrorKind::SyntaxError => NjavacOutcome::SyntaxError(diagnostic),
        },
        Err(payload) => NjavacOutcome::InternalPanic(panic_payload(&payload)),
    }
}

fn panic_payload(payload: &Box<dyn Any + Send>) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".to_string())
}

/// Deterministically prove that returned diagnostics and invariant panics remain
/// distinct without depending on malformed input to trigger compiler internals.
pub(super) fn selftest_outcome_capture() -> Result<(), String> {
    let unsupported = capture_outcome(|| {
        Err(Diagnostic::unsupported_syntax(
            Span::empty(0),
            "selftest unsupported",
        ))
    });
    if !matches!(
        classify(Some(&[]), unsupported),
        ByteOutcome::NjavacUnsupported(_)
    ) {
        return Err("unsupported diagnostic was misclassified".to_string());
    }

    let syntax = capture_outcome(|| Err(Diagnostic::parse(Span::empty(0), "selftest syntax")));
    if !matches!(
        classify(Some(&[]), syntax),
        ByteOutcome::NjavacSyntaxError(_)
    ) {
        return Err("syntax diagnostic was not classified as an invalid rejection".to_string());
    }

    let panic = capture_outcome(|| panic!("selftest injected invariant panic"));
    match classify(Some(&[]), panic) {
        ByteOutcome::NjavacInternalPanic(detail)
            if detail == "selftest injected invariant panic" => {}
        ByteOutcome::NjavacInternalPanic(detail) => {
            return Err(format!("panic payload was not preserved: {detail}"));
        }
        _ => return Err("injected panic was not classified as an internal failure".to_string()),
    }
    Ok(())
}

/// Exact-byte result after applying javac-first oracle precedence.
pub(super) enum ByteOutcome<'a> {
    GeneratorInvalid,
    NjavacUnsupported(Diagnostic),
    NjavacSyntaxError(Diagnostic),
    NjavacInternalPanic(String),
    Identical,
    Divergent { javac: &'a [u8], njavac: Vec<u8> },
}

/// Preserve the oracle's load-bearing precedence: javac rejection dominates,
/// regardless of the candidate outcome. When javac accepts, returned diagnostics
/// and internal panics remain distinct from accepted-byte comparisons.
pub(super) fn classify<'a>(javac: Option<&'a [u8]>, njavac: NjavacOutcome) -> ByteOutcome<'a> {
    let Some(javac) = javac else {
        return ByteOutcome::GeneratorInvalid;
    };

    match njavac {
        NjavacOutcome::Unsupported(diagnostic) => ByteOutcome::NjavacUnsupported(diagnostic),
        NjavacOutcome::SyntaxError(diagnostic) => ByteOutcome::NjavacSyntaxError(diagnostic),
        NjavacOutcome::InternalPanic(detail) => ByteOutcome::NjavacInternalPanic(detail),
        NjavacOutcome::Accepted(bytes) if javac == bytes => ByteOutcome::Identical,
        NjavacOutcome::Accepted(bytes) => ByteOutcome::Divergent {
            javac,
            njavac: bytes,
        },
    }
}
