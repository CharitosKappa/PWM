#!/bin/sh
# Restores a backup from scripts/backup.sh into the compose stack (D76 §3):
# stops the server, recreates the database, restores the file, gives the
# server a new epoch with `pwm-server mark-restored` (D30), which also deletes
# again the accounts deleted since the backup (D74 §6), and starts the server
# again. The deletions volume is never touched. Run from the repository root (or wherever compose.yaml is):
#
#   sh scripts/restore.sh backups/pwm-2026-11-07.dump
#
# Everything written after the backup is replaced. Devices notice the new
# epoch, pull everything again and upload what the backup lost.
set -eu

if [ "$#" -ne 1 ]; then
    echo "usage: sh scripts/restore.sh <file>" >&2
    exit 2
fi
in=$1
if [ ! -f "$in" ]; then
    echo "$in is not a file" >&2
    exit 1
fi

# Checked before anything is touched.
if ! docker compose exec -T postgres pg_restore --list <"$in" >/dev/null; then
    echo "$in is not a pg_dump backup; nothing was changed" >&2
    exit 1
fi

docker compose stop server
docker compose exec -T postgres dropdb -U pwm --force --if-exists pwm
docker compose exec -T postgres createdb -U pwm pwm
docker compose exec -T postgres pg_restore -U pwm -d pwm --no-owner --exit-on-error <"$in"

# Without a new epoch, devices can miss changes made after the restore, so
# the server starts only if this worked.
if ! docker compose run --rm --no-deps server mark-restored; then
    echo "mark-restored failed, so the server stays stopped." >&2
    echo "Fix the cause, then run: docker compose run --rm server mark-restored && docker compose up -d server" >&2
    exit 1
fi
docker compose up -d --wait server
echo "restored $in; the server has a new epoch"
