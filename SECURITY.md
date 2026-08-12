# Security & Privacy

## Privacy posture

Marginalia is local-first by design.

- No Marginalia account, no Marginalia server.
- No telemetry. No analytics. No crash reporting by default.
- No document contents, annotations, or metadata leave your machine, except to
  Zotero — and only for operations you explicitly trigger.
- All data lives in a local SQLite database and a local working directory.

The only outbound network traffic in V1 is to the Zotero API, and only if you
configure it.

## Credentials

Zotero API keys, and any device developer-access password, are stored in **OS
secure storage**:

| Platform | Store |
|---|---|
| macOS | Keychain |
| Windows | Credential Manager |
| Linux | Secret Service (libsecret) |

Never stored in SQLite, config files, environment files, or logs. Never printed
in error messages. Revocable from Settings.

## Logging

Levels: `DEBUG`, `INFO`, `WARN`, `ERROR`, `SAFETY`. The `SAFETY` channel records
every device-operation authorization decision (granted and denied), rollback,
and capability change, and is viewable in-app.

Never logged: API keys, tokens, passwords, note contents, highlight text
(sanitised context only, and only in debug builds).

## Device safety

Marginalia's device safety rules are documented in
[docs/safety/SAFETY_MODEL.md](./docs/safety/SAFETY_MODEL.md) and
[docs/safety/DEVICE_WRITE_POLICY.md](./docs/safety/DEVICE_WRITE_POLICY.md).

Summary of what Marginalia will never do to a reMarkable:

```
✗ patch or replace xochitl
✗ modify the bootloader, kernel, or any system partition
✗ replace system libraries
✗ disable or interfere with firmware updates
✗ install package managers automatically
✗ delete user documents automatically
✗ overwrite an original PDF
✗ modify documents Marginalia did not create
```

There is no setting, flag, or debug mode that enables any of the above.

## Reporting a vulnerability

Report privately rather than via a public issue. Include affected version,
platform, reproduction steps, and impact. Security issues that could damage a
user's device or data are treated as the highest priority.

Contact address: **TBD before first public release.**

## Threat model boundaries

Marginalia assumes the local machine and the connected reMarkable are trusted.
It does not defend against a compromised host OS. It does aim to defend against
its **own** bugs damaging your device or data — which is the realistic risk, and
the reason for the safety architecture.
