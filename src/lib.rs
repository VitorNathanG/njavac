//! njavac — a toy Java 25 compiler (library crate).
//!
//! Pipeline: source text -> lexer -> parser -> sema -> codegen -> class bytes.
//! For the documented supported language, the complete pipeline must preserve
//! the repository-pinned javac's behavior and retain its bytes when practical.

pub mod classfile;
pub mod classdump;
pub mod span;
pub mod diagnostic;
mod fxhash;
pub mod lexer;
pub mod ast;
pub mod parser;
pub mod sema;
pub mod codegen;

/// Compiler-owned compilation-stage markers exposed to repository tooling such
/// as the benchmark runner. Caller-owned work after `compile_observed` returns is
/// deliberately outside this enum. These names are not a stable external API.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilePhase {
    Lex,
    Parse,
    Sema,
    CodegenPlan,
    ClassfileEmit,
    Cleanup,
}

/// Repository-tooling hook around production compiler stages.
///
/// `compile` uses a statically dispatched no-op implementation, so normal
/// compilation does not execute timers or allocation counters.
#[doc(hidden)]
pub trait CompileObserver {
    fn phase_started(&mut self, phase: CompilePhase);
    fn phase_finished(&mut self, phase: CompilePhase);
}

struct NoopObserver;

impl CompileObserver for NoopObserver {
    #[inline(always)]
    fn phase_started(&mut self, _phase: CompilePhase) {}

    #[inline(always)]
    fn phase_finished(&mut self, _phase: CompilePhase) {}
}

/// Compile Java source text to `.class` bytes.
///
/// This is the one fixed contract of the front-end build: the internal module
/// boundaries and types may be redesigned freely, but this signature, its
/// behaviorally compatible class generation, and practical byte retention must
/// hold.
///
/// `source_file` is the basename used for the `SourceFile` attribute
/// (e.g. "Foo.java"); the class name itself comes from the parsed source.
pub fn compile(source: &str, source_file: &str) -> diagnostic::CompileResult<Vec<u8>> {
    compile_observed(source, source_file, &mut NoopObserver)
}

/// Compile through the production pipeline while notifying repository tooling
/// at exact compiler-owned stage boundaries.
#[doc(hidden)]
pub fn compile_observed<O: CompileObserver>(
    source: &str,
    source_file: &str,
    observer: &mut O,
) -> diagnostic::CompileResult<Vec<u8>> {
    observer.phase_started(CompilePhase::Lex);
    let tokens = lexer::lex(source);
    observer.phase_finished(CompilePhase::Lex);
    let tokens = tokens?;

    observer.phase_started(CompilePhase::Parse);
    let unit = parser::parse(tokens);
    observer.phase_finished(CompilePhase::Parse);
    let unit = unit?;

    observer.phase_started(CompilePhase::Sema);
    let analysis = sema::analyze(&unit);
    observer.phase_finished(CompilePhase::Sema);
    let analysis = analysis?;

    observer.phase_started(CompilePhase::CodegenPlan);
    let plan = codegen::plan(&unit, &analysis, source_file);
    observer.phase_finished(CompilePhase::CodegenPlan);
    let plan = plan?;

    observer.phase_started(CompilePhase::ClassfileEmit);
    let bytes = plan.to_bytes();
    observer.phase_finished(CompilePhase::ClassfileEmit);

    observer.phase_started(CompilePhase::Cleanup);
    drop(analysis);
    drop(unit);
    observer.phase_finished(CompilePhase::Cleanup);
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::{CompileObserver, CompilePhase, compile_observed};

    #[derive(Default)]
    struct Recorder(Vec<(bool, CompilePhase)>);

    impl CompileObserver for Recorder {
        fn phase_started(&mut self, phase: CompilePhase) {
            self.0.push((true, phase));
        }

        fn phase_finished(&mut self, phase: CompilePhase) {
            self.0.push((false, phase));
        }
    }

    fn observed(source: &str) -> Vec<CompilePhase> {
        let mut recorder = Recorder::default();
        assert!(compile_observed(source, "X.java", &mut recorder).is_err());
        assert_eq!(recorder.0.len() % 2, 0, "observer emitted an unmatched event");
        for events in recorder.0.chunks_exact(2) {
            assert_eq!(events[0], (true, events[0].1));
            assert_eq!(events[1], (false, events[0].1));
        }
        recorder.0.into_iter().step_by(2).map(|event| event.1).collect()
    }

    #[test]
    fn observer_reports_well_formed_error_prefixes() {
        assert_eq!(observed("@"), vec![CompilePhase::Lex]);
        assert_eq!(observed("public"), vec![CompilePhase::Lex, CompilePhase::Parse]);
        assert_eq!(
            observed("public class X { public static void main(String[] args) { int x; System.out.println(x); } }"),
            vec![CompilePhase::Lex, CompilePhase::Parse, CompilePhase::Sema],
        );
        assert_eq!(
            observed("public class X { public static void main(String[] args) { boolean a = true; boolean b = false; boolean c = a & (b == true); } }"),
            vec![
                CompilePhase::Lex,
                CompilePhase::Parse,
                CompilePhase::Sema,
                CompilePhase::CodegenPlan,
            ],
        );
    }

    #[test]
    fn observer_reports_the_complete_successful_sequence() {
        let source = "public class X { public static void main(String[] args) {} }";
        let mut recorder = Recorder::default();
        assert!(compile_observed(source, "X.java", &mut recorder).is_ok());
        assert_eq!(recorder.0.len(), 12);
        let phases: Vec<_> = recorder
            .0
            .chunks_exact(2)
            .map(|events| {
                assert_eq!(events[0], (true, events[0].1));
                assert_eq!(events[1], (false, events[0].1));
                events[0].1
            })
            .collect();
        assert_eq!(
            phases,
            vec![
                CompilePhase::Lex,
                CompilePhase::Parse,
                CompilePhase::Sema,
                CompilePhase::CodegenPlan,
                CompilePhase::ClassfileEmit,
                CompilePhase::Cleanup,
            ],
        );
    }
}
