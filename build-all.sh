#!/usr/bin/env bash
# Compile the sample with both compilers into out/, in separate subdirs so the
# file name still matches the class name (i.e. each is runnable with `java`).
#   out/javac/HelloWorld.class    <- reference, from javac
#   out/njavac/HelloWorld.class   <- ours
set -euo pipefail

JAVAC="${JAVAC:-$HOME/.sdkman/candidates/java/25.0.2-graalce/bin/javac}"
SRC="${1:-reference/HelloWorld.java}"

cargo build --release >/dev/null
rm -rf out/javac out/njavac
mkdir -p out/javac out/njavac

"$JAVAC" -d out/javac "$SRC"
./target/release/njavac out/njavac/HelloWorld.class

echo "out/javac/HelloWorld.class   $(wc -c < out/javac/HelloWorld.class) bytes"
echo "out/njavac/HelloWorld.class  $(wc -c < out/njavac/HelloWorld.class) bytes"
if cmp -s out/javac/HelloWorld.class out/njavac/HelloWorld.class; then
  echo "✅ byte-identical"
else
  echo "❌ differ:"; cmp out/javac/HelloWorld.class out/njavac/HelloWorld.class
fi
