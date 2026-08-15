# Security policy

## Reporting a vulnerability

Please do **not** open a public issue for security problems.

Report privately through
[GitHub Security Advisories](https://github.com/zaki-vemp/depot-desktop/security/advisories/new)
("Report a vulnerability" on the repository's Security tab). Enable private vulnerability
reporting in the repository settings so that link works.

Include what you can: affected version and OS, reproduction steps, and the impact you see.
Expect an acknowledgement within a few days. Depot is a small volunteer project, so fixes
land on a best-effort schedule; you will get an update either way.

## Supported versions

Depot is pre-1.0. Only the latest release on `main` receives fixes.

## What Depot touches

Understanding the attack surface helps when judging a report:

| Area | Details |
|---|---|
| Filesystem | Full read/write access to whatever the running user can reach, including delete and permanent delete |
| OAuth tokens | Google Drive refresh and access tokens in the app-data directory, plain JSON on disk, protected by OS file permissions |
| Provider credentials | Client IDs and secrets for Google, OneDrive, Dropbox and S3 in `settings.json`, same directory |
| Webviews | Website and social app tabs are real native child webviews with their own cookie store, isolated from the app's IPC surface |
| Torrents | librqbit connects to peers for links you paste; no search, no indexer |
| Network | Google Drive APIs, sites you open, torrent swarms. No telemetry, no analytics, no update pings |

## Things that are known and intentional

- Tokens and credentials are stored as plain JSON rather than in the OS keychain. Moving them
  to a keychain is on the roadmap; until then, treat the app-data directory as sensitive.
- The asset protocol scope is `**`, which lets the frontend read any file the user has already
  navigated to. This is what makes previews work.
- Permanent delete really is permanent — it does not go through Trash.
- Release builds are unsigned. Verify what you run.

## Out of scope

- Vulnerabilities in upstream dependencies (Tauri, librqbit, VLC, webview engines) — report
  those upstream; tell us too if Depot's usage makes them worse.
- Attacks that need an already-compromised machine or an attacker with the user's own file
  permissions.
