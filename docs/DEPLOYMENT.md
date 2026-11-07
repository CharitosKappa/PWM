# Deploying the server

How to run `pwm-server` outside development (decisions D25, D29, D30). The same image serves the hosted service and, later, self-hosted installs; full self-hosting docs come with M11.

## The image

`docker build -t pwm-server .` builds the image from the repository root:
- **Contents:** a static binary on `scratch`, with the Mozilla CA bundle and a non-root user (uid 10001).
- **No shell and no package manager:** nothing to log into, and no GPL software in the image.
- **Commands:**
  - `pwm-server`: serve the API (the default).
  - `pwm-server keygen`: print new server secrets.
  - `pwm-server mark-restored`: run once after restoring a backup (see below).
  - `pwm-server healthcheck`: the image's `HEALTHCHECK`.

## Settings

| Variable | Required | Meaning |
| --- | --- | --- |
| `PWM_DATABASE_URL` | yes | Postgres URL. For a database on another host, add `sslmode=verify-full`. |
| `PWM_SERVER_SECRETS` | yes | From `pwm-server keygen`. Keep it in a secret manager **with a backup**: losing it locks every account out, and it cannot be rotated in v1. |
| `PWM_LISTEN` | no | Default `0.0.0.0:8080` in the image. |
| `PWM_TRUSTED_PROXIES` | no | Addresses or CIDR ranges of your reverse proxy, such as `10.0.0.0/8`. Without it, every client looks like the proxy to the rate limits. |
| `PWM_APNS_KEY_FILE` | no | Path to the APNs `.p8` key from the Apple developer account (Keys, with Apple Push Notifications service). Mount it read-only; keep it like `PWM_SERVER_SECRETS`. Without it, push is off and the apps still sync on their own schedule (D50). |
| `PWM_APNS_KEY_ID`, `PWM_APNS_TEAM_ID` | with the key | The key's 10-character id and the team id. |
| `PWM_APNS_IOS_TOPIC`, `PWM_APNS_MACOS_TOPIC` | with the key | The iOS and macOS apps' bundle ids. |
| `PWM_APNS_URL` | no | Replaces Apple's addresses; for tests only. |
| `PWM_FCM_CREDENTIALS_FILE` | no | Path to the Firebase service account's JSON key (Firebase console, Project settings, Service accounts), for push to Android (D60). Mount it read-only; keep it like `PWM_SERVER_SECRETS`. Without it, Android push is off and the app still syncs every 15 minutes. APNs and FCM are each on or off by themselves. |
| `PWM_FCM_URL`, `PWM_FCM_TOKEN_URL` | no | Replace Google's send and token addresses; for tests only. |
| `PWM_LOG` | no | Log filter, default `info,sqlx=warn`. |
| `PWM_LOG_FORMAT` | no | `json` (default in the image) or `text`. |

The container can run with a read-only root filesystem, all capabilities dropped and `no-new-privileges`, as `compose.yaml` does.

## TLS in front: required

The server speaks plain HTTP. Put a reverse proxy in front that:
- **serves HTTPS only**, with TLS 1.2 or newer, and redirects HTTP to HTTPS
- **sends HSTS** (`Strict-Transport-Security: max-age=31536000; includeSubDomains`)
- **sets `X-Forwarded-For`** to the client address
- **is listed in `PWM_TRUSTED_PROXIES`**, so per-IP rate limits see real clients
- **doesn't log request bodies.** They contain only ciphertext, but nothing needs them.

A Caddy example (Caddy gets and renews certificates by itself):

```
vault.example.com {
    header Strict-Transport-Security "max-age=31536000; includeSubDomains"
    reverse_proxy pwm-server:8080
}
```

Caddy sets `X-Forwarded-For` by default. Set `PWM_TRUSTED_PROXIES` to the address Caddy connects from, for example the compose network's range.

## Postgres

- **Version:** 17 or newer. The server applies its migrations at startup. It refuses a database migrated by a newer server, or one whose applied migrations were edited.
- **TLS:** for a remote database, use `sslmode=verify-full`. The server checks the certificate against the image's CA bundle. A private CA can be added by mounting a bundle and setting `SSL_CERT_FILE`.

## Backups and restore

The database holds only ciphertext, ids, sizes and times, plus account emails. Keep backups as carefully as the database.

**With the compose stack,** two scripts do it from the repository root (`just backup <file>` and `just restore <file>` run the same):

```
sh scripts/backup.sh backups/pwm-2026-11-07.dump
sh scripts/restore.sh backups/pwm-2026-11-07.dump
```

- **`backup.sh`:** a `pg_dump` in custom format. It never replaces an existing file.
- **`restore.sh`:**
  1. Checks that the file is a backup.
  2. Stops the server and recreates the database from the file.
  3. Runs `pwm-server mark-restored`.
  4. Starts the server again, but only if step 3 worked.

  Everything written after the backup is replaced.
- **Tested:** CI backs up, writes more, restores, and checks that the later writes are gone and the epoch is new.

**Other setups** can use their usual tool (`pg_dump`, or snapshots plus WAL archiving). **After restoring a backup**, devices may hold changes newer than the backup. Before letting clients back in, run once:

```
docker compose run --rm server mark-restored
```

This gives the server a new epoch. Every device notices it on its next sync, pulls everything again, and uploads what the backup lost. **Skipping this step can make devices miss changes made after the restore.**

## Clean-up

The server cleans up by itself, at start-up and then every hour (D70). Each run logs one line with counts only. It deletes:
- **Sessions** whose refresh token has expired (90 days), including those of revoked devices.
- **Login attempts and device approvals** past their lifetime.
- **Autofill reports** older than 90 days.
- **Tombstones** (the markers deleted items leave) older than 180 days. A device that hasn't synced since before such a delete gets 410 `resync_required` and pulls its vaults again from the start. Items the server no longer has are then deleted on that device; its unsent edits are kept as new items.

Several servers on one database can all run it: each delete is safe to repeat.

## Health and logs

- **Health:** `GET /healthz` answers 200 when Postgres answers and 503 otherwise. It reveals nothing else about the deployment.
- **Logs:** JSON lines on stdout, one per request:
  - a request id (also sent to the client as `x-request-id`), the method, the route template, the status and the duration
  - never paths, IPs, account ids, tokens or contents.

  Every line passes through a scrubber that removes anything shaped like a token, an email address or base64 data (D29).

## Autofill failure reports

Users can send an "Autofill didn't work here" report from the extension (D38). They go to the hosted service only, anonymously: no account, device or IP address is stored. They are kept 90 days and deleted as new ones arrive. For triage:

```
docker compose exec -T server /pwm-server reports export --since 2026-11-01 > reports.jsonl
```

Each line is `{received_at, site, report}`. A self-hosted server accepts reports too, but the extension never sends it any: they always go to the hosted service.
