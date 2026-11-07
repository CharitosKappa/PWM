#!/bin/sh
# Backs up the compose stack's database (D70 §3) with pg_dump's custom
# format, through the postgres container. Run from the repository root (or
# wherever compose.yaml is):
#
#   sh scripts/backup.sh backups/pwm-2026-11-07.dump
#
# The file holds ciphertext, ids, sizes and times, plus account emails.
# Keep it like the database. Restore with scripts/restore.sh.
set -eu

if [ "$#" -ne 1 ]; then
    echo "usage: sh scripts/backup.sh <file>" >&2
    exit 2
fi
out=$1
if [ -e "$out" ]; then
    echo "$out already exists; choose a new name" >&2
    exit 1
fi

# Written under a temporary name, so a failed dump never looks like a backup.
partial="$out.partial"
trap 'rm -f "$partial"' EXIT
docker compose exec -T postgres pg_dump -U pwm -d pwm --format=custom >"$partial"
mv "$partial" "$out"
trap - EXIT
echo "backup written to $out"
