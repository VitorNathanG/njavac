# Agent Guide

This file bootstraps AI agents working in njavac. The authoritative project
knowledge is the [njavac Maintainer Guide](docs/src/index.md). Do not rebuild a
second manual here.

## Mandatory Reading

Read the pages relevant to the task before editing:

| Task | Required authority |
| --- | --- |
| Any compiler change | [Compatibility contract](docs/src/reference/compatibility-contract.md), [language support](docs/src/reference/language-support.md), and [current architecture](docs/src/architecture/overview.md) |
| Language feature | [Implementing a rung](docs/src/contributing/implementing-a-rung.md), [research method](docs/src/contributing/research-method.md), and [active work](docs/src/direction/active-work.md) |
| Byte divergence or bug | [Fixing a divergence](docs/src/contributing/fixing-a-divergence.md) and [differential debugging](docs/src/tooling/differential-debugging.md) |
| Tooling or tests | [Command surface](docs/src/tooling/command-surface.md) and the relevant page under `docs/src/tooling/` |
| Architecture | [Current architecture](docs/src/architecture/overview.md) and [architecture direction](docs/src/direction/architecture.md) |
| Planning | [Active work](docs/src/direction/active-work.md), [language rungs](docs/src/direction/language-rungs.md), and [deferred work](docs/src/direction/deferred-work.md) |
| Documentation | [Documentation policy](docs/src/contributing/documentation-policy.md), [documentation tooling](docs/src/tooling/documentation.md), and the style guide below |

Run `make help` for the exact command surface. Code and executable configuration
remain authoritative for machine behavior and exact flags.

## Non-Negotiable Rules

- The product invariant is byte identity with the repository-pinned javac for
  every supported program. Semantically equivalent bytes are not sufficient.
- Learn javac behavior only through black-box probes, raw class bytes,
  repository tools, fixtures, and fuzzing. Never inspect or decompile javac or
  OpenJDK implementation sources.
- Run acceptance tests through the sanctioned Docker-backed Make targets. Host
  `make check` and `make profile` are debugging and measurement tools, not
  correctness evidence.
- Preserve the supported surface. Never compile an unsupported case to wrong
  bytes; return a structured unsupported diagnostic when a required subsystem is
  genuinely absent. Internal invariant failures remain panics.
- Keep semantic decisions out of JVM encoding layers. Respect the ownership and
  dependency rules in the architecture guide.
- Treat constant-pool order, instruction order and physical form, members,
  attributes, frames, line events, and generated artifacts as byte-visible
  ordered data.
- Prefer the smallest correct change. Land a byte-preserving structural tidy
  separately from behavior that uses it.
- Work one bug signature or one language rung at a time. Reproduce, explain,
  fix, add a documented regression fixture, and run the final fixture-inclusive
  gates before moving on.
- Do not introduce backward-compatibility wrappers without a concrete shipped or
  persisted compatibility requirement.
- Never push when the user says not to push. Do not create branches. Commit only
  when requested or when the user has explicitly authorized incremental commits.
  Otherwise leave verified work in the working tree.
- Before committing, inspect status, diff, and recent history; stage only intended
  files. Do not amend unless explicitly requested.
- At the end of a substantial cycle, present a short reflection: what worked,
  what failed, and one concrete improvement. Add accepted non-active ideas to
  [deferred work](docs/src/direction/deferred-work.md).

## User Decisions

Use the question tool whenever the user must choose between meaningful options.
Explain the tradeoff, put the recommended option first, and do not silently make
scope, architecture, compatibility, or workflow decisions on the user's behalf.

## Documentation Style Guide

The official documentation is a maintained product used by humans and AI agents.
Every code change must leave it truthful, navigable, and internally consistent.

### Authority and Scope

- Give every durable fact one authoritative home according to the
  [documentation policy](docs/src/contributing/documentation-policy.md).
- Link to an authority instead of copying enough prose to drift.
- Keep `README.md` a concise project entrance and this file an agent bootstrap.
- Put repository-wide explanation in `docs/src/`; put one function's exact
  invariant or empirical decision beside that function as a Rust or Java doc
  comment.
- Keep executable facts such as exact flags, configured versions, and numeric
  constants in code or configuration. Documentation explains their meaning and
  points readers to the executable authority.
- Do not preserve completed work in planning pages. Delete completed active items
  and use git history as the historical record.

### Truth and Evidence

- Describe current behavior from the source and authoritative gates, not memory.
- Clearly separate current support, current mechanics, target architecture,
  active work, deferred ideas, and unimplemented research.
- Label future javac claims as observed, inferred, predicted, or unverified using
  [Evidence and Confidence](docs/src/research/evidence.md).
- Never say that behavior was "ported" from javac. Use language such as
  "empirically reconstructed from pinned black-box output."
- Call known defects defects. Do not turn accidental behavior into a supported
  contract merely because the code currently does it.
- Qualify broad claims. Prefer an exact supported list over words such as "all,"
  "complete," or "every" unless the statement is demonstrably exhaustive.

### Structure and Navigation

- Each page starts with one `#` title and a short statement of its responsibility.
- Organize pages for tasks and reader questions, not source-file chronology.
- Use descriptive headings with stable wording. Never reference numbered sections
  such as `§0.1`, line numbers, "above," or "below."
- Use relative `.md` links so links work in both GitHub source rendering and
  mdBook. Link to a heading only when the narrower target materially helps.
- Update `docs/src/SUMMARY.md` whenever a page is added, moved, or deleted. A page
  omitted from the summary is not validated or discoverable.
- Add reciprocal links where readers naturally move between contract, mechanism,
  tooling, workflow, research, and planning.

### Writing

- Write direct, factual English for a maintainer who does not already know the
  repository.
- Define project-specific terms on first use and keep terminology consistent.
- Prefer short paragraphs, tables for comparisons, and flat lists for independent
  facts. Avoid deeply nested lists.
- Use fenced code blocks with a language tag. Commands must be runnable from the
  repository root unless the text says otherwise.
- Use inline code for symbols, files, commands, diagnostics, opcodes, and literal
  values.
- Use ASCII prose by default. Non-ASCII belongs only where it is the subject of an
  example or technically necessary.
- Avoid ornamental wording, emojis, generated-sounding introductions, and claims
  without an owner or evidence path.

### Diagrams and Images

- Use Mermaid when relationships, flow, state, or ordering are easier to verify
  visually than in prose. Keep the adjacent prose sufficient for readers and
  agents that do not render diagrams.
- Keep diagrams small, directional, and consistent with the text. A diagram is
  not a substitute for naming ownership and constraints.
- Store images under `docs/src/assets/images/`, use descriptive lowercase names,
  provide useful alt text, and prefer SVG or optimized PNG. Do not commit
  screenshots when a reproducible text or Mermaid representation is clearer.
- Never hotlink an image required to understand the guide.

### Code References

- Prefer repository-relative file paths and named types/functions over brittle
  line-number links.
- Architecture pages explain component relationships; they link to decision
  functions rather than duplicating detailed opcode, frame, or folding truth
  tables.
- Correct stale code comments in the same change that moves or changes their
  authority.

### Plans and Research

- `docs/src/direction/active-work.md` contains only ordered open infrastructure
  and confirmed defects.
- `docs/src/direction/language-rungs.md` contains only language-feature order.
- `docs/src/direction/deferred-work.md` is unordered and contains only worthwhile
  non-active improvements.
- Research pages preserve unimplemented observations and confidence, not promises
  or checked-off history. When a feature lands, move its durable facts to support,
  architecture, fixtures, and code comments, then remove obsolete research prose.

### Validation

- Run `make docs-check` after every documentation change. It builds every page in
  `SUMMARY.md`, runs Mermaid preprocessing, and checks rendered internal links.
- Preview substantial layout or diagram changes with `make docs` at
  `http://localhost:3000`.
- Run `git diff --check` and search the repository for deleted paths, stale section
  references, and superseded terminology.
- When documentation infrastructure, Makefile behavior, Docker, fixtures, or
  compiler source also changes, run the relevant gates from the command-surface
  guide. Documentation-only prose does not require compiler correctness unless it
  changes or claims freshly verified compiler behavior.

## Skill

For language rungs and byte divergences, load
`.claude/skills/byte-identity-rung/SKILL.md`. The skill is an orchestration entry
point; the linked maintainer-guide pages remain authoritative.
