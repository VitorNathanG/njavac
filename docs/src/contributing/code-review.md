# Adversarial Code Review

This page defines how njavac code reviews establish scope, challenge claims, and
report findings. A review seeks the strongest correct explanation of the code,
not the largest number of comments.

## Review standard

A finding identifies a current defect or design problem with evidence, impact,
and a falsifiable account of its cause. A preference, unmeasured performance
idea, or possible future need is not a finding.

Review the system as critically as the evidence permits. Check behavior,
architecture, maintainability, invalid states, public exposure, tests,
documentation, robustness, and performance. Do not lower the standard because a
correct repair would be broad.

The recommended repair is the **correct change**: it restores the invariant at
the authority that owns it and leaves that ownership easier to understand and
extend. It may be a local edit or a staged rearchitecture. Diff size is secondary
to correctness and future maintainability. Conversely, do not prescribe a broad
rewrite when a coherent local design repairs the root cause.

Code review is read-only unless the maintainer separately requests fixes. This
keeps observed problems distinct from changes introduced during the review.

## One workflow, several lenses

Use one review workflow and one orchestration skill. Separate skills for
architecture, type design, or performance would duplicate evidence rules and
make cross-cutting findings easy to miss. Add another skill only when a task has
a distinct input, toolchain, artifact, or completion gate. Specialization within
a review belongs in review passes or subagent assignments.

Every pass uses this workflow and the current
[architecture](../architecture/overview.md). Compiler claims also require the
[compatibility contract](../reference/compatibility-contract.md) and documented
[language boundary](../reference/language-support.md). Commands and performance
claims follow the [command surface](../tooling/command-surface.md) and its linked
methodologies.

## Establish a reproducible scope

Before drawing conclusions, use the requester's scope or obtain maintainer
agreement on proposed inclusions and exclusions. Then record:

- The Git revision and a snapshot sufficient to reconstruct all staged, unstaged,
  and untracked content when the reviewed state is not committed. Fingerprint the
  retained snapshot separately.
- Included and excluded crates, directories, generated artifacts, and concerns.
- The authorities read and the exact commands, options, seeds, and retained
  artifacts used as evidence.
- A coverage ledger of files and interfaces reviewed, partially reviewed, or
  skipped.

Partition the repository by current ownership boundaries rather than arbitrary
file counts. Follow data across each boundary before judging either side. If the
worktree changes materially during review, record the new boundary or recheck the
affected conclusions.

## Review passes

Apply every relevant lens to each ownership area:

| Lens | Questions |
| --- | --- |
| Correctness and compatibility | Can supported input be rejected, miscompiled, made nondeterministic, or emitted in the wrong byte-visible form? Are edge cases and internal invariants sound? |
| Domain and phase state | Can an internal API represent a state its phase claims to have ruled out? Are strings, indexes, booleans, and independent options standing in for domain concepts? |
| Architecture and maintenance | Does each decision live with its authority? Would the next feature require duplicated policy, coordinated edits, or another exception? Is duplication real or preserving a meaningful distinction? |
| API and failure boundaries | Is visibility or mutability broader than required? Can callers bypass construction rules? Are diagnostics, unsupported input, and internal panics classified correctly? |
| Data and performance | What data dominates time or allocation? Do its layout, lifetime, traversal, and access pattern fit the measured workload? Is abstraction cost present on a relevant path? |
| Tests and documentation | Do tests protect behavior rather than implementation accidents? Do code, support, architecture, commands, and planning describe the same system? |
| Robustness | Can malformed input, overflow, resource growth, filesystem behavior, concurrency, or `unsafe` code violate an invariant or obscure a failure? |

Search for patterns, but inspect complete call paths before reporting them.
`unwrap`, cloning, public visibility, a long function, or repeated syntax is a
lead, not a finding by itself.

## Make invalid states unrepresentable

Apply "parse, don't validate" at the phase that owns each rule. Untrusted input
may remain invalid until the responsible phase diagnoses it; a syntax tree must
not pretend that semantic attribution has already succeeded. After a phase does
establish an invariant, downstream types and constructors should preserve it
without repeated checks.

Look for:

- Public fields or constructors that bypass invariants.
- Independent flags or options with illegal combinations.
- Raw strings or numeric indexes used after identity or range has been resolved.
- Repeated validation that signals a missing boundary type.
- One type reused across phases even though most of its states are legal in only
  one phase.
- Loss of source distinctions before byte-visible consumers finish with them.

Prefer the simplest representation that removes the invalid state: a closed
enum, newtype, private constructor, resolved ID, or phase-specific record. Do not
introduce typestate machinery or generic wrappers unless they eliminate a
concrete class of misuse. Modeled source errors return diagnostics; contradictions
inside an established compiler invariant remain panics.

## Review performance through data

Performance conclusions start with sanctioned measurement, not visual guesses.
Use the [benchmark and profiling methodology](../tooling/profiling.md) to identify
relevant phases, allocation sources, and workloads. Without that evidence, record
a focused investigation rather than a performance finding.

For measured hot data, record its cardinality, size, lifetime, ownership, mutation,
iteration order, and access pattern. Then examine contiguous traversal, locality,
indirection, allocation, cloning, hashing, repeated decoding, and redundant
passes. Consider arrays of structures, structures of arrays, stable IDs, arenas,
or batching only when they fit those facts and preserve ordered byte-visible
behavior.

A read-only review may diagnose measured current cost. Quantified claims about a
proposed improvement require complete correct implementations compared under
compatible benchmark reports. Microbenchmarks and instrumented phase timing can
explain a result but do not replace uninstrumented evidence. `make benchmark`
supplies performance evidence; `make test` independently establishes correctness.

## Challenge each candidate

Before accepting a finding:

1. State the violated invariant and its owner.
2. Trace a concrete path from input or caller to impact.
3. Search sibling cases and all relevant call sites.
4. Check tests, documentation, and architecture for a deliberate constraint.
5. Form the strongest alternative explanation and try to prove it.
6. Reproduce the problem or provide structural evidence that another reviewer can
   check.
7. Describe the correct change and the gates that would verify it.

Reject or downgrade a candidate when its impact is hypothetical, its evidence
does not distinguish the proposed cause, or the alleged duplication preserves a
required semantic or byte-visible difference. Mark unresolved performance and
architecture questions as investigations instead of inflating their certainty.

## Review record

Use one record per root cause or focused investigation:

```text
ID and title:
Status: confirmed defect | confirmed design problem | investigation
Severity (confirmed only) and confidence:
Known or suspected invariant owner: repository-relative path and symbol or heading
Symptom locations: repository-relative paths and symbols; snapshot lines if useful
Known or suspected invariant:
Evidence or reproducer:
Impact and reachability:
Alternative explanation tested:
Correct change (confirmed) or next discriminating step (investigation):
Verification:
```

Severity reflects behavioral impact, reachability, blast radius, and maintenance
risk, not repair size. Investigations state confidence and potential impact but
receive no severity until confirmed. Merge symptoms that share a cause, but
preserve distinct findings when their owners or repairs differ.
Use `critical` for broad silent corruption, security, or data-loss risk; `high`
for reachable contract violations or structural defects that make correct change
unsafe; `medium` for bounded defects or concrete maintenance hazards; and `low`
for local problems with limited impact. High confidence requires a reproducer,
gate failure, or complete structural proof. Medium confidence has strong evidence
with a stated unverified condition. Low-confidence claims remain investigations.

## Review lifecycle

The coordinator keeps one report in the review session or another temporary
workspace outside the tracked documentation tree. Subagents return review
records to it rather than creating competing files. The report is a task artifact,
not a new authority in the Maintainer Guide.

Present confirmed findings first, followed by investigations, coverage, commands,
material rejected hypotheses, and residual risks. The snapshot and coverage
ledger make the report reproducible without claiming that a broad review is
exhaustive.

After maintainer review:

- Confirmed defects and selected active remediation, including infrastructure and
  design work, enter [Active Work](../direction/active-work.md) in execution order.
- Accepted non-active improvements enter
  [Deferred Work](../direction/deferred-work.md).
- For a publicly reachable defect, the support or contract authority states the
  current externally observable limitation. `Active Work` owns selection, order,
  remediation scope, and completion criteria, and links to that fact instead of
  repeating its behavioral explanation.
- Rejected candidates and completed findings do not remain in project planning.
- A fixed finding survives as code, focused regression evidence, local invariant
  documentation when needed, and Git history.

Do not commit historical review reports into the guide or silently promote every
review suggestion into planned work.

## AI-assisted division of work

Use a coordinator and a staged fan-out:

1. The coordinator freezes the snapshot, reads the authorities, maps ownership,
   and creates the coverage ledger.
2. Primary reviewers receive disjoint ownership areas from the repository map.
   Each applies all review lenses and returns coverage, candidate findings,
   evidence, and material rejected hypotheses. They do not edit code.
3. Cross-cutting specialists inspect only named interfaces or measured paths,
   such as phase-state transitions, public API boundaries, or profiled data flow.
   They do not repeat a general repository review.
4. After candidates exist, challengers try to disprove them. High-impact findings
   require independent confirmation or a reproducible failure.
5. The coordinator adjudicates evidence, merges shared root causes, resolves
   severity and confidence, and owns the final report.

Initial disjoint reviews may run in parallel. Challenge depends on their results
and runs afterward. Give every subagent the same snapshot, relevant authorities,
scope, exclusions, review schema, and prohibition on edits. Assign one owner to
each candidate so parallel work does not produce competing reports.

## Completion criteria

A review is complete when the coverage ledger accounts for the agreed scope,
every reported finding has survived challenge, performance claims have sanctioned
evidence, and limitations are explicit. The final report must distinguish
confirmed defects, design problems, investigations, and maintainer decisions.
