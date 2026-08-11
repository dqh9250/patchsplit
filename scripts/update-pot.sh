#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

version="$(
    sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml |
        sed -n '1p'
)"

while IFS= read -r file || [ -n "$file" ]; do
    case "$file" in
        '' | \#*) continue ;;
    esac

    mkdir -p "$tmpdir/$(dirname "$file")"
    awk '
        function raw_start_length(line, i, j, ch) {
            if (substr(line, i, 1) != "r") {
                return 0
            }

            j = i + 1
            while (substr(line, j, 1) == "#") {
                j++
            }

            if (substr(line, j, 1) == "\"") {
                raw_hashes = j - i - 1
                raw_close = "\""
                for (ch = 0; ch < raw_hashes; ch++) {
                    raw_close = raw_close "#"
                }
                return raw_hashes + 2
            }

            return 0
        }

        function char_literal_length(line, i, j, ch, escaped) {
            if (substr(line, i, 1) != "'\''") {
                return 0
            }

            for (j = i + 1; j <= length(line); j++) {
                ch = substr(line, j, 1)
                if (escaped) {
                    escaped = 0
                } else if (ch == "\\") {
                    escaped = 1
                } else if (ch == "'\''") {
                    return j - i + 1
                } else if (ch ~ /[[:space:]]/) {
                    return 0
                }
            }

            return 0
        }

        function scan(line, i, ch, nxt, previous, tail, match_length, raw_length, char_length) {
            for (i = 1; i <= length(line); i++) {
                ch = substr(line, i, 1)
                nxt = substr(line, i + 1, 1)

                if (raw_string) {
                    if (substr(line, i, length(raw_close)) == raw_close) {
                        raw_string = 0
                        i += length(raw_close) - 1
                    }
                    continue
                }
                if (block_comment) {
                    if (ch == "*" && nxt == "/") {
                        block_comment = 0
                        i++
                    }
                    continue
                }
                if (line_comment) {
                    continue
                }
                if (quote) {
                    if (escaped) {
                        escaped = 0
                    } else if (ch == "\\") {
                        escaped = 1
                    } else if (ch == quote) {
                        quote = ""
                    }
                    continue
                }

                raw_length = raw_start_length(line, i)
                if (raw_length) {
                    raw_string = 1
                    i += raw_length - 1
                    continue
                }
                if (ch == "/" && nxt == "/") {
                    line_comment = 1
                    i++
                    continue
                }
                if (ch == "/" && nxt == "*") {
                    block_comment = 1
                    i++
                    continue
                }
                if (ch == "\"") {
                    quote = ch
                    escaped = 0
                    continue
                }
                char_length = char_literal_length(line, i)
                if (char_length) {
                    i += char_length - 1
                    continue
                }

                previous = substr(line, i - 1, 1)
                tail = substr(line, i)
                if (!capturing &&
                    (i == 1 || previous !~ /[[:alnum:]_]/) &&
                    tail ~ /^tr(_args)?[[:space:]]*\(/) {
                    match(tail, /^tr(_args)?[[:space:]]*\(/)
                    match_length = RLENGTH
                    while (substr(line, i + match_length - 1, 1) != "(") {
                        match_length--
                    }
                    capturing = 1
                    found = 1
                    depth = 1
                    i += match_length - 1
                    continue
                }

                if (capturing && ch == "(") {
                    depth++
                } else if (capturing && ch == ")") {
                    depth--
                    if (depth <= 0) {
                        complete = 1
                        break
                    }
                }
            }
            line_comment = 0
        }

        {
            was_capturing = capturing
            found = 0
            complete = 0
            scan($0)

            if (was_capturing || found) {
                print
                if (complete || depth <= 0) {
                    print ";"
                    capturing = 0
                    quote = ""
                    raw_string = 0
                    line_comment = 0
                }
            }
        }
    ' "$file" > "$tmpdir/$file"
done < po/POTFILES.in

xgettext \
    --language=C \
    --from-code=UTF-8 \
    --keyword=tr \
    --keyword=tr_args:1 \
    --package-name=patchsplit \
    --package-version="$version" \
    --msgid-bugs-address=https://github.com/zitzhen/patchsplit/issues \
    --add-location=file \
    --force-po \
    --directory="$tmpdir" \
    --files-from=po/POTFILES.in \
    --output=po/patchsplit.pot
