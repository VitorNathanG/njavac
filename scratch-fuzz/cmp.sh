#!/bin/sh
set -e
f="$1"; name=$(basename "$f" .java)
jd=$(mktemp -d); nd=$(mktemp -d)
"$JAVA_HOME/bin/javac" -d "$jd" "$f" 2>/tmp/jerr || { echo "JAVAC REJECT:"; cat /tmp/jerr; exit 3; }
njavac -d "$nd" "$f" 2>/tmp/nerr || { echo "NJAVAC REJECT:"; cat /tmp/nerr; exit 4; }
if cmp -s "$jd/$name.class" "$nd/$name.class"; then echo "IDENTICAL: $name"; else
  echo "===== DIVERGE $name ====="
  classdiff "$jd/$name.class" "$nd/$name.class" 2>/dev/null | head -20 || true
  echo "--- javac main ---"; "$JAVA_HOME/bin/javap" -c -p "$jd/$name.class" | sed -n '/void main/,/^$/p'
  echo "--- njavac main ---"; "$JAVA_HOME/bin/javap" -c -p "$nd/$name.class" | sed -n '/void main/,/^$/p'
fi
