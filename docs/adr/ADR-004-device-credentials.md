# ADR-004 — Zotero credential storage on the device

- **Status:** Proposed
- **Date:** 2026-08-12
- **Related:** invariant 13, Phase 2

## Context

Phase 0 stores the Zotero API key in the OS secure store (Keychain / Credential
Manager / Secret Service) via `keyring`. The reMarkable 2 has none of these,
and `keyring` must never appear in a portable crate — an architecture test now
enforces that.

The device therefore needs its own least-privilege credential adapter.

## Decision

Introduce a `CredentialStore` port in the portable layer:

```rust
pub trait CredentialStore {
    fn store(&self, key: CredentialKey, secret: Redacted<String>) -> Result<()>;
    fn load(&self, key: CredentialKey) -> Result<Option<Redacted<String>>>;
    fn delete(&self, key: CredentialKey) -> Result<()>;
}
```

Two implementations:

- **desktop** — the existing OS secure store, unchanged;
- **device** — a single file inside the reMarkFlow-owned data directory, mode
  `0600`, owned by the app user, never inside a native document path.

The secret crosses the boundary as `Redacted<String>`, which already renders as
`<redacted>` in both `Display` and `Debug`. That is what stops the realistic
leak: `tracing::info!(?config)` on a struct that happens to contain a key.

## Honest limitation

A `0600` file is **not** equivalent to a hardware-backed keychain. Anyone with
root on the device can read it. This must be stated plainly in the user-facing
setup flow: the key is stored locally on the device, protected by file
permissions, and should be a Zotero key scoped to the minimum permissions the
enabled features need — and revocable from Zotero at any time.

Overstating this protection would be worse than the limitation itself.

## Consequences

- Setup UI must offer revoke/reset, and revocation must clear the local file.
- Redaction tests extend to the device adapter: a test asserts the secret never
  appears in any log sink.
- No key may be embedded in builds, fixtures, screenshots, crash reports, or
  source control.
