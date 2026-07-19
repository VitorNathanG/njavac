# Troubleshooting

Start with the exact command output and classify the failure as environment,
cache, compiler, worker, observer, or documentation. Do not switch to host javac
or a local benchmark to bypass a Docker failure; that changes the reference and
invalidates the result.

## Docker image does not build

Check that Docker is running, BuildKit is available, the host can resolve base
images and download toolchains on a cold build, and enough disk space remains for
the JDK and Cargo layers. Re-run the failing Make target rather than reproducing
selected Dockerfile commands by hand; the target preserves the expected tag and
context.

A Cargo registry or target cache miss makes a build slower. The JDK and mdBook
archives are checksum-verified and base images are digest-pinned; a checksum
failure is a hard supply or version mismatch and must not be bypassed.

## A performance target rejects the CPU selection

`make benchmark` pins one CPU by host index. If Docker reports an
invalid CPU set, choose an index visible to Docker with `BENCH_CPU`. On constrained
desktop Docker VMs, the host's logical CPU numbers and Docker's available set may
differ.

Keep `BENCH_MEM` fixed between compared runs. Changing CPU or memory settings can
make the command run, but the resulting timing is not comparable to a baseline
made under different controls.

Even with unchanged controls, compare repeated nearby samples on the same host.
Docker CPU and memory controls do not eliminate host scheduling, virtualization,
power, thermal, or background-load variance. The benchmark is controlled, not
deterministic.

The benchmark fails when a warm-up, measured compiler, profiled compilation,
allocation invariant, or publication fails. Measured compiler failures already
include compiler identity, scenario,
warm-up or sample number, the exact command, original status, diagnostic replay
status, and replay stderr. Use that diagnostic rather than reconstructing a hidden
command. If a successful timing remains implausibly short, inspect the raw samples
in the applicable exploratory JSON under `benchmark-results/`.

Run `make test` for deterministic report parsing, phase sequencing,
allocation/resource protocols, CLI behavior, exact-byte correctness, fuzzer
infrastructure, and documentation validation. Run
`make benchmark-help` to inspect the effective modes and controls without relying
on a host-built binary.

## `make verify` fails unexpectedly

The golden volume can be stale. `make verify` auto-records only when the volume has
no class files; it does not detect fixture or JDK changes in a nonempty cache.

Run:

```sh
make record
make correctness
```

`make record` refreshes the complete suite from the configured javac. `make correctness`
then removes the cache from the diagnosis by comparing against a fresh invocation.
If fresh correctness still fails, treat it as a real divergence and follow
[Differential Debugging](../tooling/differential-debugging.md).

If every fixture fails only in offline mode, stale, incomplete, or incompatible
goldens are especially likely. Never repair a golden by copying or editing a class
file manually.

Recording rewrites entries for current fixtures but does not remove goldens for
renamed or deleted fixtures. Such an orphan is ignored by comparison, yet can keep
the volume nonempty and suppress `make verify`'s automatic recording. Remove the
whole Docker volume when a strictly clean cache is needed, then run `make record`.

## `record FILE=...` updates more than one case

This is intentional. The `record` target does not pass `FILE` to its recording
step, so the configured javac records the whole suite. `FILE` filters only the offline
verification that follows. See [Fixtures and Goldens](../tooling/fixtures-and-goldens.md).
That following verification is already part of `make record`; an immediate
`make verify` is redundant.

## A focused fixture command cannot see an ad hoc source

`verify` and `correctness` use sources copied into the acceptance image and do not
bind-mount the repository. Their `FILE` mode is intended for fixtures present in
that image. `benchmark` rejects `FILE`. Use `make src-diff FILE=...` for an ad hoc
source under the repository; that target mounts the working tree.

Each command rebuilds or re-evaluates its capability-image dependency first, so a
newly added fixture should become visible to acceptance commands. If
it does not, inspect the selected Dockerfile target, build context, and ignore
rules before changing the harness.

## `src-diff` succeeded but printed a mismatch

This is expected behavior. `src-diff` is a diagnostic command: when both compilers
accept, it ignores the nonzero statuses from `classdiff` and textual `diff` so all
diagnostics print. It therefore returns success for byte divergence and can also
hide a structural or text diagnostic-tool failure. Inspect all diagnostic output,
not only the final status.

Read `IDENTICAL` versus `bytes differ` in its output. Use
`make correctness FILE=fixtures/.../Case.java` or `make correctness` when the exit
status must enforce byte identity.

Reference and njavac rejection use distinct statuses inside the container shell,
but GNU Make generally returns its own recipe-failure status. Use the printed
`javac rejected` or `njavac rejected` label rather than scripting against those
inner numeric values.

`make diff` has a related ambiguity: zero means the two supplied classes are
identical, while nonzero can mean either divergence or a read/parse/usage failure.
Read the `classdiff` output before classifying the result.

## `javap` agrees but bytes differ

`javap` is not a complete byte renderer. Trust the byte comparison and inspect the
structural `classdiff` path and offset. Constant-pool ordering, attribute bytes, or
other representational differences can remain invisible in disassembly. Use
`make diff` for two retained classes or the report printed by the fixture harness.

## Fuzzer workers cannot start

The dedicated fuzz image contains `FuzzJavac.java` and `FuzzObserve.java` under
`/opt/njavac/tools` and sets absolute worker paths. A missing-worker error therefore
indicates a stale or incorrectly built fuzz image, an explicit path override, or a
direct invocation that did not use the `fuzz` Dockerfile target.

Run the corresponding `make fuzz...` target from the repository root so Make
re-evaluates that target. If worker paths were explicitly overridden, remove the
override and use the paths baked into the image.

The Make targets do not forward arbitrary host environment variables into the
container. Host `JAVA`, `JAVAC`, `FUZZ_WORKER`, and `FUZZ_OBSERVER` values affect a
direct host binary but not these recipes unless a recipe explicitly adds them.

## `make fuzz` exits zero with byte divergences

The fuzzer's operational hard-fail condition is behavioral difference, invalid
njavac syntax rejection, or panic. A byte divergence whose observer output matches
is telemetry, so the process can exit zero.

Matching observations satisfy the fuzzer's behavioral check for its current
generated subset, while the physical difference remains byte-retention telemetry.
Record the seed and structural signature and reduce the case. If the reference
form is practical to retain, add an exact fixture after fixing it. Only the
compatibility contract's optimization exception permits an alternate
representation, and it requires a sanctioned durable behavioral regression oracle
appropriate to the affected surface. Until that oracle exists, the drift remains
a candidate rather than supported behavior. Normal byte-only telemetry does not
write a finding bundle, so preserve the printed example before starting another
run.

## Fuzzer output is missing

Hard findings normally persist under ignored `fuzz-out/` because Make bind-mounts
that directory into the fuzz image. Byte-only behavior matches print telemetry but
do not write a normal artifact bundle. Unsupported and worker mismatch dumps are
capped, so the summary count may exceed the number of saved examples.

With the default stop-on-finding mode, the run exits after its first hard finding.
Use the binary's documented keep-going mode through `FUZZFLAGS` only when a census
is useful; always retain the printed seed.

The fuzzer container runs as root, so persisted bind-mount output can be
root-owned. Only repository-root `fuzz-out/` is mounted and ignored. Put a custom
`--out-dir` below `fuzz-out/`; another container path is ephemeral and disappears
with `--rm`.

## A fuzz run hangs, passes without cases, or ignores a value

Use positive decimal `COUNT` and `BATCH` values and verify the printed run header.
`COUNT=0` can produce a vacuous success. `BATCH=0` with a positive count prevents
progress. Some malformed named values silently retain a default or consume the
next option, so parser acceptance does not prove the requested controls were used.

A seed alone is not a complete reproduction record. Keep the commit, image/JDK,
worker and observer sources, count, batch, flags, stop mode, and printed header.

## Observer times out or restarts

Timeout is a modeled observation, not automatically a worker crash. The observer
allows two seconds per class, marks the unrun peer when necessary, and the Rust
driver restarts the JVM before continuing or reversing the pair. Run
`make fuzz-observe-verify` if timeout recovery itself is suspect.

The observer is valid only for the current generated subset. Do not enable fuzz
generation of input reads, `System.exit`, threads, or persistent global mutation
without redesigning the isolation boundary.

## Worker verification diverges

Treat a `make fuzz-verify` failure as an invalid fuzzer oracle, not an njavac
compiler finding. Inspect artifacts under `fuzz-out/worker-mismatch/` and compare
CLI acceptance and bytes with worker output. Changes to the JDK, virtual source
name, compiler options, batching, in-memory file manager, or worker protocol can
all invalidate the worker.

Do not rely on normal fuzz results until the worker agrees with the configured CLI
over the selected verification sample.

Worker verification covers only the generated sample selected by its seed, count,
batch, and current generator. A pass is evidence for that sample, not proof over
all programs or batching shapes. Unexpected worker rejection can also mask a
caught Java compiler `RuntimeException`, because the worker suppresses diagnostics
and returns a partial class set.

## The fuzzer exits with no panic diagnostic

The fuzzer suppresses the process-wide Rust panic hook so captured candidate
panics are not printed twice. This also hides the normal message for uncaught
harness and infrastructure panics. A silent status such as 101 can indicate a bad
path, filesystem error, worker failure, protocol invariant, or harness defect; it
is not automatically an njavac compiler finding. Recheck mounts and ownership,
then isolate the relevant worker or direct binary mode.

## Image build or performance is green

`make image` proves only that the pinned acceptance build completed. The
performance sections of `make benchmark` measure speed and resources; they are not
compatibility evidence. The benchmark has no compatibility section; run `make
test` for deterministic correctness and infrastructure evidence.

If benchmark numbers move sharply on macOS, verify that both runs used the same
power mode and comparable thermal/background conditions. Low Power Mode and
regular performance mode are not comparable baselines.

## Documentation page is absent from the book

mdBook renders pages listed by `docs/src/SUMMARY.md`. A build alone can omit an
unlisted source, but `make docs-check` now inventories all recursive Markdown
sources and fails when one lacks a summary entry. Add the page to navigation and
rerun the full gate.

If `make docs` cannot bind its port, override `DOCS_PORT`. The server publishes on
loopback only. If generated files have wrong ownership, use the Make target rather
than a root-run direct container; the target supplies the host UID/GID.

The configured `mdbook-mermaid` is built against mdBook 0.5.4 through the committed
lockfile. Treat a preprocessor version warning as a toolchain mismatch and repair
the pinned build rather than suppressing it.

## CI is green but another gate was not run

The hosted workflow runs `make test` on pushes and pull requests. It does not run
random-seed fuzz campaigns or live performance measurement. Run those explicitly
when the change requires them. See
[Docker and CI](../tooling/docker-and-ci.md) for the current automation boundary
and [Command Surface](../tooling/command-surface.md) for gate selection.
