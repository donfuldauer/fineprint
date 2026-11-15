#!/usr/bin/env bash
# This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
# If a copy of the MPL was not distributed with this file, You can obtain one at
# https://mozilla.org/MPL/2.0/.

# FP-005 (#6) spike hash parity (REQ-NFR-003).
#
# Usage: scripts/parity.sh <vectors.txt> <results-dir> <required-file>...
#
# 1. Recomputes every expected digest in vectors.txt with sha256sum (an independent tool, not the
#    toy library) and fails if the file's expected column disagrees.
# 2. Builds the expected caller output ("<name> <hex>\n" per input).
# 3. Fails unless every required result file exists, every *.txt in <results-dir> is
#    byte-identical to the expected output, and no unexpected file is present.

set -euo pipefail

if [ "$#" -lt 3 ]; then
  echo "usage: $0 <vectors.txt> <results-dir> <required-file>..." >&2
  exit 2
fi
vectors=$1
results=$2
shift 2

expected=$(mktemp)
trap 'rm -f "$expected"' EXIT

while read -r name input digest extra; do
  [ -z "${name:-}" ] && continue
  if [ -z "${digest:-}" ] || [ -n "${extra:-}" ]; then
    echo "FAIL: malformed vectors line for '$name'" >&2
    exit 1
  fi
  if [ "$input" = "-" ]; then
    actual=$(printf '' | sha256sum | cut -d' ' -f1)
  else
    actual=$(python3 -I -c 'import sys; sys.stdout.buffer.write(bytes.fromhex(sys.argv[1]))' "$input" \
      | sha256sum | cut -d' ' -f1)
  fi
  if [ "$actual" != "$digest" ]; then
    echo "FAIL: vectors.txt expects $digest for '$name' but sha256sum gives $actual" >&2
    exit 1
  fi
  printf '%s %s\n' "$name" "$digest" >>"$expected"
done <"$vectors"

if [ ! -s "$expected" ]; then
  echo "FAIL: no vectors in $vectors" >&2
  exit 1
fi
echo "expected (independently recomputed with sha256sum):"
cat "$expected"

status=0
for required in "$@"; do
  if [ ! -f "$results/$required" ]; then
    echo "FAIL: missing result file $required" >&2
    status=1
  fi
done

count=0
for file in "$results"/*.txt; do
  [ -e "$file" ] || continue
  base=$(basename "$file")
  known=0
  for required in "$@"; do
    [ "$required" = "$base" ] && known=1
  done
  if [ "$known" -ne 1 ]; then
    echo "FAIL: unexpected result file $base" >&2
    status=1
    continue
  fi
  if cmp -s "$expected" "$file"; then
    echo "OK   $base"
    count=$((count + 1))
  else
    echo "FAIL $base differs from the expected digests:" >&2
    diff "$expected" "$file" >&2 || true
    status=1
  fi
done

echo "$count of $# caller outputs are byte-identical to the expected digests"
exit "$status"
