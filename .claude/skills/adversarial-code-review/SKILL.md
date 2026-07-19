---
name: adversarial-code-review
description: >-
  Reproducible adversarial code review for correctness, architecture,
  maintainability, invalid states, API exposure, tests, documentation, robustness,
  and data-oriented performance. Use for general reviews, codebase audits, design
  criticism, parse-don't-validate analysis, and performance-structure reviews.
---

# Adversarial Code Review

This skill orchestrates the authoritative human workflow. It does not own review
policy or compiler mechanics.

Before reviewing, read:

1. [`docs/src/contributing/code-review.md`](../../../docs/src/contributing/code-review.md)
2. [`docs/src/architecture/overview.md`](../../../docs/src/architecture/overview.md)
3. [`docs/src/direction/architecture.md`](../../../docs/src/direction/architecture.md)

For compiler claims, also read the
[`compatibility contract`](../../../docs/src/reference/compatibility-contract.md)
and [`language support`](../../../docs/src/reference/language-support.md). For
commands or performance claims, read the
[`command surface`](../../../docs/src/tooling/command-surface.md) and its linked
methodology.

Then follow the review workflow end to end:

- Keep review separate from repair unless the maintainer requests both.
- Use the requested scope, or obtain agreement on proposed scope and exclusions.
  Then freeze a reconstructible snapshot and coverage ledger before judging code.
- Review coherent ownership areas with every lens; use specialists only for named
  cross-cutting interfaces or measured paths.
- Treat searches and metrics as leads. Trace call paths and challenge every
  candidate before reporting it.
- Resolve evidentiary ambiguity through challenge and discriminating evidence.
  Report unresolved candidates as investigations. Ask the maintainer only when
  scope, intent, policy, architecture, or workflow requires a decision.
- Require sanctioned evidence for performance claims and reason from data shape,
  lifetime, layout, and access pattern.
- Recommend the correct change at the correct authority. Never preserve a flawed
  boundary merely to minimize the diff, and never prescribe speculative
  architecture without a concrete responsibility.
- If using subagents, fan out disjoint primary scopes first, challenge candidates
  second, and let one coordinator adjudicate the final report.
- Present confirmed findings first in the documented schema, then investigations,
  coverage, evidence commands, rejected material hypotheses, and residual risks.
- Complete the review only when the coverage ledger accounts for the established
  scope, every reported finding has survived challenge, and limitations are
  explicit.

Do not inspect javac/OpenJDK implementation sources, treat an unmeasured
performance suspicion as a finding, equate repetition with semantic duplication,
or trade future maintainability for a local feature or fix.
