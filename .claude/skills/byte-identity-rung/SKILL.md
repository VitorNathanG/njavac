---
name: byte-identity-rung
description: >-
  Workflow adapter for implementing a language rung or fixing a byte divergence
  in njavac under the byte-identical-to-javac invariant. Use for new operators,
  literals, statements, types, control flow, class-file structures, fixture
  mismatches, and fuzz findings.
---

# Byte-Identity Work

This skill orchestrates the authoritative human workflow. It does not duplicate
compiler mechanics.

Before editing, read:

1. [`docs/src/reference/compatibility-contract.md`](../../../docs/src/reference/compatibility-contract.md)
2. [`docs/src/reference/language-support.md`](../../../docs/src/reference/language-support.md)
3. [`docs/src/contributing/research-method.md`](../../../docs/src/contributing/research-method.md)
4. [`docs/src/contributing/implementing-a-rung.md`](../../../docs/src/contributing/implementing-a-rung.md) for a feature, or [`docs/src/contributing/fixing-a-divergence.md`](../../../docs/src/contributing/fixing-a-divergence.md) for a bug
5. [`docs/src/direction/active-work.md`](../../../docs/src/direction/active-work.md)

Then follow that workflow end to end:

- Establish the supported or proposed boundary.
- Build durable black-box evidence before encoding a hidden javac choice.
- Keep structural preparation separate from behavior.
- Add edge-focused fixtures with globally unique matching basenames.
- Use `make help` and the [command guide](../../../docs/src/tooling/command-surface.md) to select cached, fresh, fuzz, worker, observer, and timing gates correctly.
- Update the authoritative support, architecture, research, and planning pages in
  the same change; delete completed planning entries.
- Run `make docs-check` and every behavior gate required by the workflow.
- Present the end-of-cycle reflection to the user and record only accepted,
  non-active improvements in [deferred work](../../../docs/src/direction/deferred-work.md).

Never inspect javac/OpenJDK implementation sources, accept semantically equivalent
but different bytes, or silently narrow a rung to avoid a reachable case.
