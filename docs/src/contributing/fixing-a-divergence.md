# Fixing a Divergence

A byte difference for an agreed supported program is a byte-retention divergence
that requires classification. It is a compatibility defect when either class is
invalid or relevant behavior differs. It is a candidate acceptable representation
only when reference-compiler optimization obscures the physical choice or makes it
impractical to reconstruct, and only after a sanctioned durable regression oracle
establishes equivalent behavior. Work one independently reproducible signature
through completion before starting another.

## Classify the finding

Findings can originate from a fixture, `src-diff`, classdiff, or the differential
fuzzer. Preserve the distinction between:

| Outcome | Maintainer interpretation |
| --- | --- |
| Both accept, bytes differ | Byte-retention divergence requiring structural and behavioral triage. |
| Bytes differ, relevant observations match | Candidate optimization exception; retain as telemetry until a sanctioned durable oracle covers the affected behavior. |
| Bytes and observations differ | Observed-behavior defect. |
| javac accepts, njavac returns `Unsupported` | Coverage telemetry unless the input is inside the agreed support contract. |
| javac accepts, njavac returns a syntax diagnostic | Invalid candidate rejection. |
| javac accepts, njavac panics | Internal compiler defect. |
| javac rejects | Invalid generator/probe input for compatibility purposes. |

Execution observation is evidence, not universal proof of semantic equivalence.
Use evidence appropriate to the physical surface that changed; the current fuzzer
observer is the sanctioned behavioral check only for its modeled generated subset.
See [Fuzzing](../tooling/fuzzing.md) for the oracle's process-exit and artifact
policy.

## Reproduce from the pinned environment

Record the source, seed if generated, command, and first structural signature.
Reproduce with Docker-backed repository targets, not host compilers:

```sh
make src-diff FILE=Probe.java
make fuzz SEED=<seed>
```

Read classdiff's first substantive structural field before following cascading
`javap` differences. Confirm cache freshness when the report came from
`make verify`. The detailed tool interpretation belongs to
[Differential debugging](../tooling/differential-debugging.md).

## Minimize without changing the bug

Reduce the source to the smallest program that preserves the relevant predicate:

- Both compilers still accept when acceptance is part of the finding.
- The same structural byte signature remains for a byte-retention investigation.
- The same observation difference remains for a behavioral finding.
- The same diagnostic or panic category remains for a compiler finding.

Do not use byte-only minimization for a behavioral finding if it can drift to an
observation-equivalent mismatch. Raw fuzzer output is evidence, not a ready-made
fixture. Hand-reduce incidental declarations, initializers, prints, and branches.

## Infer the rule

Apply the [black-box research method](research-method.md) to the minimized case.
Probe sibling cells and surrounding contexts before coding. A correct fix should
follow from one rule that explains the complete table, not from a predicate named
after the failing fixture.

Stop if the expanded corpus contradicts the current model or causes broad new
divergences. Return to the last verified boundary and redesign rather than adding
exceptions.

## Implement one fix

Keep the change as small as the inferred rule permits. If structural preparation
is necessary, land a behavior-preserving tidy separately under existing gates.

At the changed decision function, add or update a concise doc comment containing:

- The observed javac-compatible rule.
- The non-obvious boundary or sibling case that makes the rule necessary.
- A link or stable name for the evidence corpus or regression fixture.

Do not duplicate the same rule in general architecture prose. Architecture should
only explain which component owns the decision.

## Add the regression

Every bug fix lands with a regression test in the same change. An exact-byte
fixture must:

- Be the smallest clear program that exercises the repaired edge.
- Have a globally unique filename matching its public class.
- Live in the topical fixture directory.
- State at the top which exact-byte edge it protects and how output previously
  diverged.
- Avoid unrelated coverage that obscures the regression.

`fixtures/folding/NanCanon.java` is the established style for a fuzzer-found
regression. Follow [Fixtures and goldens](../tooling/fixtures-and-goldens.md) for
the full fixture contract.

An intentionally nonidentical representation under the optimization exception
requires a sanctioned durable behavioral oracle because the current fixture
harness is strict. Until that oracle exists, the divergence cannot become accepted
support. Keep the test focused and cover every behavior the changed physical
surface can affect.

Refresh cached goldens after adding an exact fixture:

```sh
make record
```

`make record` performs the offline verification after recording. Use a focused
`make verify FILE=...` later only when another edit needs the fast cached loop.

## Verify completion

The minimum completion sequence is:

1. The focused exact-byte fixture passes, or the sanctioned durable behavioral
   regression for an accepted alternate representation passes.
2. `make correctness` passes over the full suite against fresh pinned `javac`.
3. The original probe or seed no longer reproduces the behavioral defect; accepted
   physical drift remains documented telemetry.
4. `make fuzz` no longer reports the target behavioral signature over the agreed
   verification run.
5. Worker or observer gates pass if those components changed.
6. The code decision comment and authoritative docs are current.

Only after this sequence should the finding be removed from
[Active work](../direction/active-work.md). Do not annotate it as fixed or preserve
a completed backlog story. The lasting record is the code comment, focused
regression, durable evidence, and git commit.

## Land one bug

Do not combine independent fuzzer signatures or opportunistic refactors in one fix.
Finish reproduction, model, fix, regression, authoritative verification, planning
deletion, and reflection for one signature before beginning the next.

Commit and push authorization is governed solely by
[Maintainer Workflow](workflow.md#commit-and-push-authority). Completing the fix
does not authorize either action.
