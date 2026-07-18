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

## Benchmark rejects the CPU selection

`make bench` pins one CPU by host index. If Docker reports an invalid CPU set,
choose an index visible to Docker with `BENCH_CPU`. On constrained desktop Docker
VMs, the host's logical CPU numbers and Docker's available set may differ.

Keep `BENCH_MEM` fixed between compared runs. Changing CPU or memory settings can
make the command run, but the resulting timing is not comparable to a baseline
made under different controls.

Even with unchanged controls, compare repeated nearby samples on the same host.
Docker CPU and memory controls do not eliminate host scheduling, virtualization,
power, thermal, or background-load variance. The benchmark is controlled, not
deterministic.

If a timing sample is implausibly short, rerun the compiler with visible output.
The benchmark performs a correct fresh comparison first, but its later warm-up
and measured invocations discard output and ignore process failure status. A later
failed invocation can still be timed and reported.

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

`verify`, `correctness`, and `bench` use sources copied into the main image and do
not bind-mount the repository. Their `FILE` mode is intended for fixtures present
in that image. Use `make src-diff FILE=...` for an ad hoc source under the
repository; that target mounts the working tree.

All main targets rebuild or re-evaluate the image dependency first, so a newly
added fixture should become visible. If it does not, inspect Docker build context
and ignore rules before changing the harness.

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

Errors naming `tools/FuzzJavac.java` or `tools/FuzzObserve.java` usually mean the
fuzz binary was run in the image without the repository mount or from the wrong
working directory. The main image deliberately does not contain those Java source
files.

Run the corresponding `make fuzz...` target from the repository root. It mounts
the repository at `/w` and sets `/w` as the container workdir. If worker paths were
explicitly overridden, restore paths visible inside that mount.

The Make targets do not forward arbitrary host environment variables into the
container. Host `JAVA`, `JAVAC`, `FUZZ_WORKER`, and `FUZZ_OBSERVER` values affect a
direct host binary but not these recipes unless a recipe explicitly adds them.

## `make fuzz` exits zero with byte divergences

The fuzzer's operational hard-fail condition is behavioral difference, invalid
njavac syntax rejection, or panic. A byte divergence whose observer output matches
is telemetry, so the process can exit zero.

This does not make the drift acceptable: exact bytes are the product contract.
Record the seed and structural signature, reduce the case, and add a fixture once
the mismatch is understood. Normal byte-only telemetry does not write a finding
bundle, so preserve the printed example before starting another run.

## Fuzzer output is missing

Hard findings normally persist under ignored `fuzz-out/` because Make bind-mounts
the repository. Byte-only behavior matches print telemetry but do not write a
normal artifact bundle. Unsupported and worker mismatch dumps are capped, so the
summary count may exceed the number of saved examples.

With the default stop-on-finding mode, the run exits after its first hard finding.
Use the binary's documented keep-going mode through `FUZZFLAGS` only when a census
is useful; always retain the printed seed.

The fuzzer container runs as root, so persisted bind-mount output can be
root-owned. Only repository-root `fuzz-out/` is ignored. Put a custom `--out-dir`
below that directory or add an intentional ignore rule; otherwise generated
findings can appear as untracked source-tree files.

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

## Local build or profile is green

`make check` proves only that a host release build completed. `make profile`
measures local in-process performance. Neither invokes the configured javac or compares
bytes, so neither is acceptance evidence. Run `make correctness` separately.

If profile numbers move sharply on macOS, verify that both runs used the same
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

mdBook 0.5.4 currently warns that `mdbook-mermaid` was built against mdBook 0.5.0.
The configured build is known to complete with this warning. Do not mistake it for
the configured mdBook version or suppress it; investigate if Mermaid processing
or the build actually fails.

## CI is green but another gate was not run

The hosted workflow runs only `make correctness` on pushes and pull requests. It
does not run documentation, fuzz, worker, observer, benchmark, local build, or
profile targets. Run those explicitly when the change requires them. See
[Docker and CI](../tooling/docker-and-ci.md) for the current automation boundary
and [Command Surface](../tooling/command-surface.md) for gate selection.
