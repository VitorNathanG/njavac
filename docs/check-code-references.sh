#!/bin/sh
set -eu

root=${1:-.}
docs_dir=${2:-docs/src}
map=docs/code-references.tsv
exceptions=docs/code-reference-exceptions.tsv
forbidden=docs/forbidden-terms.tsv
references=$(mktemp)
docs_sources=$(mktemp)
term_sources=$(mktemp)
trap 'rm -f "$references" "$docs_sources" "$term_sources"' 0 HUP INT TERM

cd "$root"

if ! find "$docs_dir" -type f -name '*.md' -print > "$docs_sources"; then
    printf '%s\n' 'cannot inventory Markdown sources for code-reference checking' >&2
    exit 1
fi
LC_ALL=C sort -o "$docs_sources" "$docs_sources"

: > "$references"
while IFS= read -r source; do
    if ! awk '
        function marker_prefix(text, marker, count) {
            count = 0
            while (substr(text, count + 1, 1) == marker) count++
            return count
        }

        {
            text = $0
            content = text
            quote_depth = 0
            while (1) {
                quote_spaces = 0
                while (substr(content, quote_spaces + 1, 1) == " " && quote_spaces < 3) quote_spaces++
                if (substr(content, quote_spaces + 1, 1) != ">") break
                quote_depth++
                content = substr(content, quote_spaces + 2)
                if (substr(content, 1, 1) == " " || substr(content, 1, 1) == "\t") {
                    content = substr(content, 2)
                }
            }

            if (fence_marker != "" && quote_depth < fence_quote_depth) {
                fence_marker = ""
                fence_length = 0
                fence_quote_depth = 0
            }

            indent_chars = 0
            indent_columns = 0
            while (indent_columns < 4) {
                indentation = substr(content, indent_chars + 1, 1)
                if (indentation == " ") {
                    indent_columns++
                } else if (indentation == "\t") {
                    indent_columns += 4 - (indent_columns % 4)
                } else {
                    break
                }
                indent_chars++
            }

            if (indent_columns >= 4) {
                if (fence_marker != "") next
                if (index(content, "`") != 0) {
                    printf "%s:%d: backticks in indented content are unsupported; use a fenced block\n", FILENAME, FNR > "/dev/stderr"
                    failed = 1
                }
                next
            }
            text = content
            trimmed = substr(content, indent_chars + 1)

            if (fence_marker != "") {
                marker_count = marker_prefix(trimmed, fence_marker)
                if (marker_count >= fence_length && substr(trimmed, marker_count + 1) ~ /^[[:space:]]*$/) {
                    fence_marker = ""
                    fence_length = 0
                    fence_quote_depth = 0
                }
                next
            }

            backticks = marker_prefix(trimmed, "`")
            tildes = marker_prefix(trimmed, "~")
            if (backticks >= 3 || tildes >= 3) {
                if (backticks >= 3) {
                    if (index(substr(trimmed, backticks + 1), "`") != 0) {
                        printf "%s:%d: invalid backtick fence info string\n", FILENAME, FNR > "/dev/stderr"
                        failed = 1
                        next
                    }
                    fence_marker = "`"
                    fence_length = backticks
                } else {
                    fence_marker = "~"
                    fence_length = tildes
                }
                fence_quote_depth = quote_depth
                next
            }

            inline = 0
            token = ""
            invalid = 0
            for (position = 1; position <= length(text); position++) {
                character = substr(text, position, 1)
                following = substr(text, position + 1, 1)
                if (!inline && character == "\\") {
                    backslashes = 1
                    while (substr(text, position + backslashes, 1) == "\\") backslashes++
                    if (substr(text, position + backslashes, 1) == "`") {
                        if (backslashes % 2 == 1) {
                            position += backslashes
                        } else {
                            position += backslashes - 1
                        }
                        continue
                    }
                }
                if (character == "`") {
                    if (following == "`") {
                        printf "%s:%d: double-backtick inline code is unsupported; use paired single backticks or a fenced block\n", FILENAME, FNR > "/dev/stderr"
                        failed = 1
                        invalid = 1
                        break
                    }
                    if (inline) {
                        if (token != "") printf "%s\t%d\t%s\n", FILENAME, FNR, token
                        token = ""
                        inline = 0
                    } else {
                        inline = 1
                    }
                    continue
                }
                if (inline) token = token character
            }
            if (!invalid && inline) {
                printf "%s:%d: multiline or unmatched inline code is unsupported\n", FILENAME, FNR > "/dev/stderr"
                failed = 1
            }
        }

        END { exit failed }
    ' "$source" >> "$references"; then
        exit 1
    fi
done < "$docs_sources"

if ! find docs/src crates -type f \( -name '*.md' -o -name '*.rs' \) -print > "$term_sources"; then
    printf '%s\n' 'cannot inventory terminology-check sources' >&2
    exit 1
fi
LC_ALL=C sort -o "$term_sources" "$term_sources"

failed=0

if ! awk -F '\t' '
    $1 != "" && $1 !~ /^#/ {
        if (NF < 3 || $2 == "" || $3 == "") {
            printf "invalid Rust reference mapping: %s\n", $0 > "/dev/stderr"
            invalid = 1
        }
        if (++seen[$1] > 1) {
            printf "duplicate Rust reference mapping: %s\n", $1 > "/dev/stderr"
            invalid = 1
        }
    }
    END { exit invalid }
' "$map"; then
    failed=1
fi

while IFS="$(printf '\t')" read -r token path anchor; do
    case "$token" in
        ''|'#'*) continue ;;
    esac
    if [ ! -f "$path" ]; then
        printf '%s\n' "mapped Rust path does not exist: $token -> $path" >&2
        failed=1
    elif ! grep -Fq "$anchor" "$path"; then
        printf '%s\n' "mapped Rust anchor is absent: $token -> $path: $anchor" >&2
        failed=1
    fi
    if ! awk -F '\t' -v token="$token" \
        '$3 == token { found = 1 } END { exit !found }' "$references"; then
        printf '%s\n' "stale Rust reference mapping: $token" >&2
        failed=1
    fi
done < "$map"

if ! awk -F '\t' '
    $1 != "" && $1 !~ /^#/ {
        if (NF < 3 || $2 == "" || $3 == "") {
            printf "invalid code-reference exception: %s\n", $0 > "/dev/stderr"
            invalid = 1
        }
        key = $1 "\t" $2
        if (++seen[key] > 1) {
            printf "duplicate code-reference exception: %s: %s\n", $1, $2 > "/dev/stderr"
            invalid = 1
        }
    }
    END { exit invalid }
' "$exceptions"; then
    failed=1
fi

while IFS="$(printf '\t')" read -r document token reason; do
    case "$document" in
        ''|'#'*) continue ;;
    esac
    if [ ! -f "$document" ]; then
        printf '%s\n' "code-reference exception document does not exist: $document" >&2
        failed=1
    elif ! awk -F '\t' -v document="$document" -v token="$token" \
        '$1 == document && $3 == token { found = 1 } END { exit !found }' \
        "$references"; then
        printf '%s\n' "stale code-reference exception: $document: $token" >&2
        failed=1
    fi
done < "$exceptions"

while IFS="$(printf '\t')" read -r document line token; do
    if awk -F '\t' -v document="$document" -v token="$token" \
        '$1 == document && $2 == token { found = 1 } END { exit !found }' \
        "$exceptions"; then
        continue
    fi

    case "$token" in
        njavac::*|njavac_compiler::*|njavac_classdump::*)
            mapped_path=$(awk -F '\t' -v token="$token" \
                '$1 == token { print $2; exit }' "$map")
            if [ -z "$mapped_path" ]; then
                printf '%s:%s: unmapped public Rust reference `%s`\n' \
                    "$document" "$line" "$token" >&2
                failed=1
            fi
            continue
            ;;
    esac

    path=${token%%::*}
    case "$path" in
        ./*) path=${path#./} ;;
    esac
    case "$path" in
        crates/*|docs/*|fixtures/*|tools/*|src/*|.github/*|.claude/*|Cargo.toml|Cargo.lock|Dockerfile|Makefile|README.md|CLAUDE.md)
            ;;
        *) continue ;;
    esac

    case "$token" in
        *' '*|*"$(printf '\t')"*)
            printf '%s:%s: path-like inline code contains whitespace; use a fenced block or an exception: `%s`\n' \
                "$document" "$line" "$token" >&2
            failed=1
            continue
            ;;
    esac

    case "$path" in
        */...|*/.../*) continue ;;
        *'*'*|*'?'*|*'['*)
            case "$token" in
                *::*)
                    printf '%s:%s: symbols on repository globs are unsupported: `%s`\n' \
                        "$document" "$line" "$token" >&2
                    failed=1
                    continue
                    ;;
            esac
            set -- $path
            if [ "$1" = "$path" ] && [ ! -e "$1" ]; then
                printf '%s:%s: repository glob matches nothing: `%s`\n' \
                    "$document" "$line" "$token" >&2
                failed=1
            fi
            continue
            ;;
    esac

    case "$path" in
        */)
            if [ ! -d "$path" ]; then
                printf '%s:%s: repository directory does not exist: `%s`\n' \
                    "$document" "$line" "$token" >&2
                failed=1
            fi
            ;;
        *)
            if [ ! -e "$path" ]; then
                printf '%s:%s: repository path does not exist: `%s`\n' \
                    "$document" "$line" "$token" >&2
                failed=1
                continue
            fi
            ;;
    esac

    case "$token" in
        *::*)
            symbol=${token#*::}
            symbol=${symbol##*::}
            symbol=${symbol%%(*}
            symbol=${symbol%%<*}
            case "$symbol" in
                ''|*[!A-Za-z0-9_]*)
                    printf '%s:%s: unsupported file-qualified Rust symbol: `%s`\n' \
                        "$document" "$line" "$token" >&2
                    failed=1
                    ;;
                *)
                    if [ -f "$path" ] && ! grep -Eq "(^|[^[:alnum:]_])${symbol}([^[:alnum:]_]|$)" "$path"; then
                        printf '%s:%s: symbol `%s` is absent from `%s`\n' \
                            "$document" "$line" "$symbol" "$path" >&2
                        failed=1
                    fi
                    ;;
            esac
            ;;
    esac
done < "$references"

if ! awk -F '\t' '
    $1 != "" && $1 !~ /^#/ {
        if (NF < 2 || $2 == "") {
            printf "forbidden terminology has no replacement guidance: %s\n", $0 > "/dev/stderr"
            invalid = 1
        }
        if (++seen[$1] > 1) {
            printf "duplicate forbidden terminology: %s\n", $1 > "/dev/stderr"
            invalid = 1
        }
    }
    END { exit invalid }
' "$forbidden"; then
    failed=1
fi

while IFS="$(printf '\t')" read -r phrase replacement; do
    case "$phrase" in
        ''|'#'*) continue ;;
    esac
    while IFS= read -r source; do
        case "$source" in
            *.md)
                if grep -Fn "$phrase" "$source" >/dev/null; then
                    printf '%s: forbidden terminology `%s`; %s\n' \
                        "$source" "$phrase" "$replacement" >&2
                    failed=1
                fi
                ;;
            *.rs)
                if awk -v phrase="$phrase" '
                    {
                        comment = $0
                        sub(/^[[:space:]]*/, "", comment)
                        line_doc = substr(comment, 1, 3) == "//!" ||
                            (substr(comment, 1, 3) == "///" && substr(comment, 4, 1) != "/")
                        if (line_doc && index(comment, phrase)) found = 1
                    }
                    END { exit !found }
                ' "$source"; then
                    printf '%s: forbidden terminology in Rust doc comments `%s`; %s\n' \
                        "$source" "$phrase" "$replacement" >&2
                    failed=1
                fi
                ;;
        esac
    done < "$term_sources"
done < "$forbidden"

exit "$failed"
