/// Compile `src` in-process, catching a panic (out-of-scope input). `None` means
/// njavac rejected. The `source_arg` must match the class and filename token.
pub(super) fn njavac_compile(src: &str, source_arg: &str) -> Option<Vec<u8>> {
    let src = src.to_string();
    let arg = source_arg.to_string();
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| njavac::compile(&src, &arg)))
        .ok()
        .and_then(Result::ok)
}

/// Exact-byte result for one source accepted or rejected by the two compilers.
/// A future behavioral observer attaches only to `Divergent`; classification
/// itself remains independent of observation policy.
pub(super) enum ByteOutcome<'a> {
    GeneratorInvalid,
    NjavacReject,
    Identical,
    Divergent { javac: &'a [u8], njavac: Vec<u8> },
}

/// Preserve the oracle's load-bearing precedence: javac rejection dominates,
/// followed by njavac rejection, exact identity, and finally byte divergence.
pub(super) fn classify<'a>(javac: Option<&'a [u8]>, njavac: Option<Vec<u8>>) -> ByteOutcome<'a> {
    match (javac, njavac) {
        (None, _) => ByteOutcome::GeneratorInvalid,
        (Some(_), None) => ByteOutcome::NjavacReject,
        (Some(a), Some(b)) if a == b => ByteOutcome::Identical,
        (Some(a), Some(b)) => ByteOutcome::Divergent { javac: a, njavac: b },
    }
}
