#!/bin/sh
# Fetches the current public suffix list and its test vectors once, from
# the newest commit of github.com/publicsuffix/list (the repository
# publicsuffix.org publishes from), checks them, regenerates the lookup
# table and prints the version change.
#
# usage: tools/update-psl.sh   (from the repository root)
set -eu

repo=publicsuffix/list
dir=crates/madcawl-net/psl
table=crates/madcawl-net/src/psl/table.rs

[ -f tools/gen-psl.py ] || { echo "run from the repository root" >&2; exit 2; }
tmp=$(mktemp -d)
trap 'rm -rf "${tmp:?}"' EXIT

curl -fsS --max-time 30 "https://api.github.com/repos/$repo/commits/main" -o "$tmp/commit.json"
sha=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["sha"])' "$tmp/commit.json")
date=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["commit"]["committer"]["date"])' "$tmp/commit.json")
raw="https://raw.githubusercontent.com/$repo/$sha"
curl -fsS --max-time 60 "$raw/public_suffix_list.dat" -o "$tmp/public_suffix_list.dat"
curl -fsS --max-time 60 "$raw/tests/test_psl.txt" -o "$tmp/test_psl.txt"

grep -q "checkPublicSuffix('example.com', 'example.com');" "$tmp/test_psl.txt"
{
    echo "source: https://github.com/$repo"
    echo "commit: $sha"
    echo "commit-date: $date"
    echo "list: $raw/public_suffix_list.dat"
    echo "tests: $raw/tests/test_psl.txt"
    (cd "$tmp" && shasum -a 256 public_suffix_list.dat test_psl.txt) | sed 's/^/sha256: /'
} > "$tmp/SOURCE"
# the generator refuses a list without both sections or with unreadable rules
python3 tools/gen-psl.py "$tmp" > "$tmp/table.rs"

old=$(sed -n 's/^commit-date: //p' "$dir/SOURCE" 2>/dev/null || true)
mkdir -p "$dir"
mv "$tmp/public_suffix_list.dat" "$tmp/test_psl.txt" "$tmp/SOURCE" "$dir/"
mv "$tmp/table.rs" "$table"
echo "public suffix list: ${old:-none} -> $date (commit $sha)"
