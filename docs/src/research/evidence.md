# Evidence and Confidence

The research pages preserve the project's broad Java 25 survey and byte-identity
hazards for language that njavac does not yet support. They are a research queue,
not a promise of syntax acceptance and not an exhaustive account of Java 25.

Current accepted behavior is authoritative only in
[Language Support](../reference/language-support.md). The reference compiler and
byte-identity boundary are defined by the
[Compatibility Contract](../reference/compatibility-contract.md).

## Confidence labels

Every research claim uses one of these labels:

| Label | Meaning |
| --- | --- |
| **[O] Observed** | A registered, checked-in probe corpus or fixture shows the pinned compiler producing the stated output. |
| **[I] Inferred** | One explicit rule explains a complete documented observation matrix. |
| **[P] Predicted** | An inferred rule predicts the result, but that result has not been independently probed. |
| **[U] Unverified** | A useful report, hypothesis, or concern lacks durable pinned evidence and must be reprobed before implementation. |

An isolated manual experiment is not promoted to **[O]** unless its input and
relevant output facts are retained. Specifications can explain semantics and
legal class-file forms, but not which legal byte sequence the pinned javac emits.

## Status of the migrated survey

The **[U]** leads came from the former root `README.md` coverage survey introduced
by the git-history entry titled `Document the Java 25 language surface still to
implement`. That survey reported manual checks with throwaway programs against the
then-pinned compiler. Neither the source probes nor their output records were
retained. The maintainer-guide migration preserved the leads but lowered their
confidence to **[U]**. This records provenance without treating a commit hash,
unretained terminal work, or the former page as current evidence.

Research pages may contain many **[U]** rows and no **[O]** row for a distant
feature. That accurately distinguishes a survey lead from evidence ready to drive
byte-compatible implementation.

## Research map

| Topic | Page | Scope |
| --- | --- | --- |
| Structural consequences | [Class-file impact](classfile-impact.md) | Frames, pool kinds, attributes, bootstrap methods, and cross-cutting byte hazards |
| Values and operations | [Values and expressions](values-and-expressions.md) | Literals, references, arrays, calls, operators, lambdas, and concatenation |
| Statements | [Control flow](control-flow.md) | Loops, switch families, abrupt completion, exceptions, synchronization, and scopes |
| Program declarations | [Declarations and types](declarations-and-types.md) | Members, inheritance, nested types, interfaces, enums, records, annotations, and generics |
| Source-set structure | [Compilation units](compilation-units.md) | Packages, imports, multiple outputs, local types, package info, and modules |
| Recent language | [Modern Java](modern-java.md) | Java 8 through Java 25 headline features and preview stamping |

The map is intentionally broad but not exhaustive. Missing Java grammar or
class-file behavior must not be interpreted as cheap, byte-invisible, or covered.

## Evidence registry

The registry currently contains fixtures only. There is no checked-in probe corpus,
so no future-language claim on the research pages is currently **[O]**, **[I]**, or
**[P]**.

| Evidence ID | Kind | Location | Canonical replay |
| --- | --- | --- | --- |
| `fixtures/current-support` | Acceptance fixtures for supported behavior | `fixtures/` | `make correctness` |

The fixture suite is durable **[O]** evidence because the Docker correctness gate
recompiles each fixture with the pinned reference and byte-compares it. Its topical
areas are:

- `basics/`
- `branches/`
- `compound-assign/`
- `conversions/`
- `folding/`
- `literals/`
- `operators/`
- `println/`
- `scopes/`
- `types/`

Fixtures guard support; a complete probe corpus justifies a hidden model. A
boundary case can belong in both, but the prose conclusion should have one home
and link to the evidence rather than duplicating a disassembly.

## Future probe corpora

The first durable research corpus must create `probes/<corpus-id>/`; later corpora
must use the same root. Use a stable lowercase, hyphenated ID that names the
decision being investigated rather than a ticket or date. Each corpus contains:

| Path | Required content |
| --- | --- |
| `README.md` | Question, competing hypotheses, matrix dimensions, omitted dimensions with reasons, conclusions by confidence label, and exact replay commands. |
| `cases/<case-id>/<PublicClass>.java` | Minimal source inputs under stable lowercase, hyphenated case IDs; each Java filename still matches its public class. |
| `observations.md` | One row per case recording the relevant raw-byte ranges or structural fields, generated artifacts, and independently checked predictions. |

Do not check in only normalized `javap` prose when the claim concerns exact bytes.
Keep generated bulk output out of the corpus unless it is the smallest durable
record of the observation.

Replay every listed case from the repository root with its manifest command. Use
the pinned reference command for unimplemented language:

```sh
make probe FILE=probes/<corpus-id>/cases/<case-id>/<PublicClass>.java
```

This replays observations exposed by the pinned verbose disassembly. A claim about
raw bytes that the command does not expose cannot become **[O]** until the manifest
names a repository-sanctioned replay that exposes those bytes; a retained
transcription alone is not enough.

Once both compilers accept the case, also use the byte and structural comparison:

```sh
make src-diff FILE=probes/<corpus-id>/cases/<case-id>/<PublicClass>.java
```

The manifest must enumerate concrete commands for all cases rather than relying on
an undocumented shell loop. A corpus becomes registered only when this page adds
its evidence ID, location, replay command, covered matrix, and resulting confidence
claim.

## Evidence record

A future corpus should record:

- The question and competing hypotheses.
- Pinned compiler identity through repository configuration.
- Minimal source inputs and exact command surface used.
- Relevant raw bytes or structural fields, not only normalized `javap` text.
- Dimensions covered and intentionally omitted.
- Observations, inferred rule, risky predictions, and independent prediction
  checks.
- Fixture links for supported boundary cases.

Useful dimensions include operand/result types, constants versus locals, encoding
boundaries, branch polarity, grouping, nesting, empty versus non-empty stack,
attribute presence/order, constant-pool order, and generated-artifact order.

Apply the canonical [black-box research loop](../contributing/research-method.md#research-loop)
when creating or revising a corpus. If a probe contradicts the current model, stop
and rebuild the model from the complete corpus rather than stacking local
exceptions onto a disproven explanation.

## Black-box boundary

Allowed evidence includes pinned repository probes, raw bytes, structural
classdiff output, pinned `javap`, fresh fixture comparisons, and differential
fuzzer observations. javac/OpenJDK source, decompilation, implementation internals,
a host JDK, intuition, or one unretained example are not authorities.

Names in njavac that resemble compiler concepts describe local models inferred
from outputs. They do not grant permission to use reference-compiler internals as
design documentation.
