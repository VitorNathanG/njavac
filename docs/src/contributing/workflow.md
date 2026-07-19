# Maintainer Workflow

njavac requires equivalent behavior for every supported program and retains the
reference class bytes whenever practical. Small, independently understandable
changes are safer than broad fixes because a plausible local rule can hide a
wrong model elsewhere in the corpus.

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

When behavior work needs structural preparation, make the smallest
behavior-preserving tidy first. Verify it under the existing gates before changing
language behavior.

The tidy and behavior change must remain independently reviewable and
independently committable. When commits are requested, land them as separate
commits. Do not hide module moves, renaming, broad formatting, or abstraction work
inside the feature or fix that motivated it.

Avoid speculative architecture. Add a module or abstraction only when a concrete
responsibility has arrived. Long-term boundaries and their triggers live in
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
