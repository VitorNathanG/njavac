use crate::javac::{derive_java, worker_src_path, JavacWorker};
use crate::observe::{observer_src_path, ObserveWorker, Termination};
use crate::Config;

/// Exercise the observer's semantic comparison and lifecycle boundaries with
/// pinned-javac classes, including a timeout followed by a successful restart.
pub(crate) fn verify_observer(cfg: &Config) -> i32 {
    const NAME: &str = "FuzzObserveProbe";

    let java = derive_java(&cfg.javac);
    let javac_src = worker_src_path();
    let observer_src = observer_src_path();
    let mut javac = JavacWorker::spawn(&java, &javac_src);
    let mut observer = ObserveWorker::spawn(&java, &observer_src);

    let reference = compile_probe(
        &mut javac,
        NAME,
        "System.out.println(\"reference\");",
    );
    let candidate = compile_probe(
        &mut javac,
        NAME,
        "System.out.println(\"candidate\");",
    );
    let same = observer.observe_pair(NAME, &reference, &reference);
    assert_eq!(same.reference, same.candidate, "observer disagreed on identical classes");
    assert_eq!(same.reference.termination, Termination::Returned);
    assert_eq!(same.reference.stdout, b"reference\n");

    let different = observer.observe_pair(NAME, &reference, &candidate);
    assert_ne!(different.reference, different.candidate, "observer missed stdout divergence");

    let mut invalid = candidate.clone();
    invalid[0] ^= 0xff;
    let invalid_pair = observer.observe_pair(NAME, &reference, &invalid);
    assert_eq!(invalid_pair.reference.termination, Termination::Returned);
    assert_eq!(invalid_pair.candidate.termination, Termination::LoadFailed);

    let throwing = compile_probe(
        &mut javac,
        NAME,
        "throw new ArithmeticException(\"probe\");",
    );
    let throws = observer.observe_pair(NAME, &throwing, &throwing);
    assert_eq!(throws.reference, throws.candidate);
    assert_eq!(throws.reference.termination, Termination::Threw);

    let looping = compile_probe(&mut javac, NAME, "while (true) {}");
    let timeout = observer.observe_pair(NAME, &looping, &looping);
    assert_eq!(timeout.reference.termination, Termination::TimedOut);
    assert_eq!(timeout.candidate.termination, Termination::TimedOut);

    let candidate_timeout = observer.observe_pair(NAME, &reference, &looping);
    assert_eq!(candidate_timeout.reference.termination, Termination::Returned);
    assert_eq!(candidate_timeout.candidate.termination, Termination::TimedOut);

    let restarted = observer.observe_pair(NAME, &reference, &reference);
    assert_eq!(restarted.reference, restarted.candidate, "observer failed after timeout restart");

    println!("verify-observer done: return/difference/load-failure/throw/timeout/restart all pass");
    0
}

fn compile_probe(worker: &mut JavacWorker, name: &str, body: &str) -> Vec<u8> {
    let source = format!(
        "public class {name} {{ public static void main(String[] args) {{ {body} }} }}"
    );
    let classes = worker.compile_batch(&[(name, &source)]);
    assert_eq!(classes.len(), 1, "observer probe javac emitted an unexpected class set");
    classes
        .get(name)
        .cloned()
        .expect("observer probe rejected by javac")
}
