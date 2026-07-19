# Maintainer Workflow

njavac requires equivalent behavior for every supported program and retains the
reference class bytes whenever practical. Independently understandable changes
are safer because a plausible local rule can hide a wrong model elsewhere in the
corpus. A correct change may still require broad structural preparation; split it
into verifiable steps instead of preserving the wrong boundary to keep a diff
small.

## Establish the boundary

Before extending an uncommitted change:

1. Inspect `git status` and the complete diff.
2. Identify the last verified commit and the gates that established it.
3. Classify the task as behavior, bug fix, infrastructure, documentation, or a
   behavior-preserving tidy required by one of those.
4. Read the authoritative support, architecture, active-work, and workflow pages
   relevant to the task.
5. State one falsifiable behavior or structure hypothesis for the cycle.

Do not revert, overwrite, or fold unrelated worktree changes into the task. If an
unrelated change does not conflict, leave it alone. If it directly invalidates the
current task boundary, stop and reconcile ownership before continuing.

## Tidy first

When behavior work needs structural preparation, establish the correct ownership
and invariants with behavior-preserving changes first. Verify them under the
existing gates before changing language behavior. Do not trade future
maintainability for a local feature or fix.

The tidy and behavior change must remain independently reviewable and
independently committable. When commits are requested, land them as separate
commits. Do not hide module moves, renaming, broad formatting, or abstraction work
inside the feature or fix that motivated it.

Diff size is not a design criterion: repair a flawed boundary when the behavior
cannot fit it coherently, but do not build abstractions for responsibilities that
have not arrived. Long-term boundaries and their triggers live in
[Architecture direction](../direction/architecture.md); active structural work
lives in [Active work](../direction/active-work.md).

## One hypothesis per cycle

Use the canonical [black-box research loop](research-method.md#research-loop) for
the evidence and model portion of a cycle. After the model survives its
discriminating probes, make any prerequisite tidy separately, implement one
behavior change, run the required gates, update authorities, and reflect.

Do not stack local patches onto a disproven model. Warning signs include a broad
new divergence census, sibling cases that require unrelated exceptions, or probes
that contradict the proposed abstraction. Return to the last verified boundary,
expand the corpus, and redesign.

## No behavioral concessions

A supported construct must preserve the pinned `javac` behavior for every
reachable case, not only the easy or common cases. Retain exact bytes whenever
practical. Complexity discovered during research is not a reason to narrow a rung
or accept a physical divergence outside the compatibility contract's narrow
optimization exception.

Refuse an input only when it genuinely requires an unbuilt subsystem or falls
outside the agreed language boundary. Such input must receive an `Unsupported`
diagnostic before class-file emission; it must never compile to invalid or
behaviorally incorrect output.
Record the boundary in [Language support](../reference/language-support.md) and,
when it blocks scheduled work, in the appropriate direction page.

Internal invariant failures remain panics. Do not weaken an assertion or convert
an invalid or behaviorally wrong path into apparent support merely to make a probe
compile.

## Format through the pinned toolchain

Run `make fmt` after editing Rust. It is the only sanctioned mutating formatting
command: it applies the repository's committed rustfmt policy through the pinned
Docker toolchain and runs as the host UID/GID. Do not run a host `cargo fmt` or
`rustfmt`; a different release or style edition can rewrite unrelated files.

Before committing Rust changes, run `make fmt-check` and the applicable acceptance
gates. Every Docker-backed njavac workspace build depends on the same non-mutating
formatting check, and `make test` names it explicitly, so CI rejects an unformatted
workspace even when the focused command was skipped. Formatting is mechanical;
review the resulting diff and do not combine unrelated normalization with behavior
work.

## Grow the fuzzer with the compiler

Every compiler peculiarity established by a rung, divergence fix, or black-box
probe must become a deterministic fuzzer scenario before the work is complete.
A peculiarity is a distinct byte-visible or behavior-visible compiler decision,
including boundary transitions, method-wide modes, physical instruction forms,
synthetic control-flow artifacts, and accepted or rejected source shapes. Random
generation is not evidence that the scenario will be reached routinely.

Express the scenario through the typed generator model when possible. Reserve a
stable scheduled case when random generation cannot reliably reach the required
shape or scale, and continue consuming the normal random case so later generated
programs retain their established seed mapping. A scheduled case is guaranteed
coverage: rejection by javac, an njavac unsupported diagnostic, syntax failure,
internal panic, or behavioral mismatch must fail the fuzz run rather than enter
an expected-rejection tally.

The exact fixture remains the authoritative regression oracle. The fuzzer case
complements it by exercising the same compiler peculiarity routinely across the
differential pipeline. Document the relationship in the generator and in the
[fuzzing guide](../tooling/fuzzing.md).

## Acceptance gates

All compiler builds, executions, and acceptance testing run through Docker-backed
Make targets. Direct host toolchains are outside the sanctioned workflow. The
exact target surface belongs to `make help`; gate purposes belong to
[Command surface](../tooling/command-surface.md).

At minimum:

`make test` is the aggregate deterministic pass/fail gate. The narrower rows below
identify useful focused checks, but a completed repository change runs the
aggregate unless the maintainer explicitly narrows the scope. `make benchmark` is
separate performance evidence and must not contain or replace correctness tests.

| Change | Required evidence |
| --- | --- |
| Documentation-only | `make docs-check`; compiler gate if Docker/build context changes |
| Behavior-preserving compiler tidy | `make correctness` |
| Language behavior | Focused fresh comparison, refreshed fixture cache, `make correctness`, and in-scope fuzzing |
| Bug fix | Exact fixture or sanctioned behavioral regression, `make correctness`, and proof that the behavioral fuzz signature is gone when applicable |
| JDK or javac-worker change | `make fuzz-verify` in addition to correctness |
| Observer change | `make fuzz-observe-verify` |
| Benchmark implementation or report contract | `make test` and a reduced `make benchmark` measurement smoke run |
| Performance or profiling claim | The applicable uninstrumented or instrumented section of `make benchmark` |

Use `make verify` for a fast cached loop, but refresh with `make record` after
fixture or JDK changes. A cached pass is not the fresh pre-commit gate.

## Commit and push authority

This repository works directly on `main`; do not create feature branches.

In a collaborative session, create a commit only when the user or responsible
maintainer explicitly authorizes a commit. Authorization to edit does not imply
authorization to commit. Keep each commit focused and stage only the intended
files. Before committing, inspect status, the full diff, and recent history, then
run the required gates.

Push only when the user or responsible maintainer explicitly authorizes a push.
Authorization to edit or commit does not imply authorization to push; without push
authorization, leave verified commits local. Never force-push, skip hooks, rewrite
unrelated history, or amend a commit unless that specific action was explicitly
authorized.

## Documentation in the cycle

Documentation and code move together. Update the one authoritative home for each
changed fact in the same commit as the behavior or infrastructure change. Do not
copy the fact into entry points, planning pages, and architecture prose.

Use [Documentation policy](documentation-policy.md) to select the owner. Completed
active work and fixed findings are deleted from planning pages; their lasting
record is code, local decision comments, fixtures, durable evidence, and git
history.

## End-of-cycle reflection

When a feature, fix, or infrastructure cycle is complete, reflect before starting
another cycle:

- What went well and should be repeated?
- What went badly, caused rework, or exposed a wrong assumption?
- What concrete tool, documentation, test, skill, or refactor would improve the
  next cycle?

Present improvements as proposals to the project owner. Do not silently expand the
completed task. Accepted active prerequisites belong in [Active work](../direction/active-work.md);
accepted non-blocking ideas belong in [Deferred work](../direction/deferred-work.md).
Durable working rules belong in this guide, not only in a conversation.
