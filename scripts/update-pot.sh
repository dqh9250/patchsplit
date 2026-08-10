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
        function scan(line, i, ch) {
            for (i = 1; i <= length(line); i++) {
                ch = substr(line, i, 1)
                if (ch == "(") {
                    depth++
                } else if (ch == ")") {
                    depth--
                }
            }
        }

        !capturing && /(^|[^[:alnum:]_])tr(_args)?[[:space:]]*\(/ {
            capturing = 1
            depth = 0
        }

        capturing {
            print
            scan($0)
            if (depth <= 0) {
                print ";"
                capturing = 0
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
