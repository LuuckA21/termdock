#!/bin/sh
# Fails when a Linux binary needs a newer glibc than the given version, so a
# release never raises the oldest supported distribution by accident.
# Usage: scripts/glibc-max.sh <binary> <max-version>
set -eu
bin=$1
max=$2
need=$(objdump -T "$bin" | grep -o 'GLIBC_[0-9.]*' | sed 's/^GLIBC_//' | sort -uV | tail -n 1)
echo "$bin needs glibc $need (at most $max allowed)"
if [ "$(printf '%s\n%s\n' "$need" "$max" | sort -V | tail -n 1)" != "$max" ]; then
  echo "::error::$bin needs glibc $need, newer than $max"
  exit 1
fi
