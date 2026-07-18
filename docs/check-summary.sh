#!/bin/sh
set -eu

summary=docs/src/SUMMARY.md
missing=$(mktemp)
trap 'rm -f "$missing"' 0 HUP INT TERM

find docs/src -type f -name '*.md' ! -path "$summary" -print \
    | LC_ALL=C sort \
    | while IFS= read -r source; do
        relative=${source#docs/src/}
        if ! grep -Fq "]($relative)" "$summary"; then
            printf '%s\n' "$source"
        fi
    done > "$missing"

if [ -s "$missing" ]; then
    printf '%s\n' 'documentation pages absent from docs/src/SUMMARY.md:' >&2
    while IFS= read -r source; do
        printf '  %s\n' "$source" >&2
    done < "$missing"
    exit 1
fi
