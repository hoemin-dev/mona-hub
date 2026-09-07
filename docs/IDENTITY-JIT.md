# MonaHub → AC/DC identity/JIT

## Runtime contract

`/app/app.js` → `MonaSession.resolve()` → argument-free `sync_acdc_identity`
→ native WebView cookie → HTTPS Access get-identity → Entra normalization
→ authenticated loopback `POST /api/me` → resolve/JIT → `{tenant_id, person_id, status}`
→ `window.monaIdentity`.

The bridge URL remains `http://127.0.0.1:8787/api/me`. Rust verifies the main
window, exact HTTPS origin and `/app/` path. It reads the HttpOnly credential
directly from the WebView and sends it only to the fixed Access HTTPS origin.
Browser-supplied identity arguments are not trusted. Redirects are disabled.
The local request bypasses proxies and contains only tid, oid and initial name,
plus the shared bridge authentication header. No credential is returned to JS.

## Why these keys

Cloudflare's get-identity response is not an Entra ID token. `oidc_fields` holds
configured custom claims; `name` is also a top-level field. The old code required
all three fields under oidc_fields, so absent custom tid/oid claims stopped IPC.
The supplied diagnostic proves those expected fields were absent; it does not
prove the contents of other fields. The live response has not been dumped.

Entra `(tid, oid)` identifies the directory object across applications and can be
used by a future Graph Scan without rematching names/emails. AC/DC maps tid to a
registered tenant and uses UNIQUE `(tenant_id, entra_oid)` to preserve person_id.
Cloudflare user_uuid identifies an Access user; idp.id identifies an IdP
integration. Neither is an Entra object ID. Changing IdP integrations must not
silently remap persons. A different Entra object remains a different identity.
No email matching, fabricated OIDs or automatic identity merges are performed.

Sources:
- https://developers.cloudflare.com/cloudflare-one/integrations/identity-providers/entra-id/
- https://developers.cloudflare.com/cloudflare-one/integrations/identity-providers/generic-oidc/
- https://developers.cloudflare.com/cloudflare-one/tutorials/extend-sso-with-workers/
- https://learn.microsoft.com/en-us/entra/identity-platform/id-token-claims-reference

## Required Cloudflare configuration

In the Zero Trust account that actually owns the MonaHub Access application:

1. Open Integrations → Identity providers → the existing Microsoft Entra IdP.
2. Add `tid` and `oid` to **OIDC Claims**, preserving existing claims/settings.
   `name` can also be added; top-level name is already supported and name is optional.
3. Save, use IdP Test, then perform a fresh MonaHub logout/login so the cached
   Access identity is replaced. Do not log or share the raw identity/token.
4. Rust's safe diagnostic must stop reporting `identity-claims-missing`.

The currently available Wrangler account contains the MonaHub Pages project,
but its account-level Access app and IdP API lists were empty during inspection.
No speculative IdP creation or change was made. Confirm the correct Zero Trust
account/integration if those lists do not match the dashboard.

## Session behavior and failures

- Read `window.monaIdentity` for `{tenant_id, person_id}` or null.
- Read `window.monaIdentityState` for status/error code; listen to
  `mona:identity-changed` for changes.
- Duplicate calls in one document share a promise; a ready result is reused.
- `window.retryMonaIdentity()` retries after an unavailable result. An online
  event also retries; starting AC/DC alone may not emit that event, so explicitly
  retry or log in again. There is no continuous profile sync or retry storm.
- Logout/pagehide clears memory and invalidates pending replies. Native checks
  reject a changed session before/after the local request. A request already
  accepted by AC/DC may still record an access during logout, but its result
  cannot populate a cleared document session.
- Access lookup timeout: 10 seconds. Local bridge timeout: 3 seconds.
- Bridge offline/timeout leaves Cloudflare login intact and Mona identity null.
  Apps needing person_id must wait for ready, rather than use another identifier.
- Invalid/missing claims, wrong tenant and inactive person fail closed for AC/DC.
- Existing display_name is unchanged by login; access time/count are updated.
  Initial missing name is stored as an empty string. Administrator Scan remains
  the future place for profile updates; no Graph scan runs at login.

## Data and diagnostics

No migration or production DB mutation is needed. Existing persons and test
rows are preserved. Tests use separate temporary DBs and tokens.
AC/DC stores tenant/object/person IDs, initial display name and access metadata;
it does not store Access tokens, cookies or full identity objects.

The existing diagnostic IPC permission/module is retained until real interactive
verification is complete, including compatibility with the previously deployed
frontend. The new frontend does not call the temporary diagnostic command.
Native operational logs contain fixed error codes, field presence and HTTP
status only; the existing diagnostic file also records resolve success/failure.

Same identity resolves consistently against the same AC/DC database. Independent
per-device SQLite databases do not provide a shared global person_id registry.
Multi-device production requires one authoritative AC/DC directory service/DB.

## Build, deploy and manual verification

MonaHub uses a remote Pages URL, including `tauri:dev:access`. Building the binary
does **not** deploy `web/`. Deploy the updated web directory to the existing
`mona-hub` Pages project and use the updated desktop binary together. The IPC
response changed from a string to an object; update both sides.

```powershell
cd D:\work\AC-DC
npm test
npm run local:start

cd D:\work\mona-hub
npm test
npm run check
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri:build
```

After claim configuration and web/binary updates:
User A login → AC/DC Users refresh → note person_id → logout/login A → same ID
→ logout/login B → different ID. Verify an actual user, not the historical
`Local Bridge Verification` row. Do not initialize/reset the existing DB.

Automated tests cover normalization, session deduplication/logout/retry, native
HTTP responses/timeout/offline, real AC/DC HTTP JIT/relogin/concurrent creation,
unchanged name, malformed identity, wrong tenant/token, inactive state and UNIQUE.
Interactive Entra/WebView cookie behavior still requires the above live test.

## Verification results (2026-09-07)

- MonaHub `npm test`: 3 passed; `npm run check`: passed.
- Rust `cargo check --offline`: passed; `cargo test --offline`: 20 passed.
- AC/DC `npm test` from the actual project: 10 passed, including isolated real HTTP.
- AC/DC capability main-window/origin/command check and both Git diff checks: passed.
- `cargo build --release --offline` and `npm run tauri:build -- --ci`: passed.
  Installer: `src-tauri/target/release/bundle/nsis/MONA-HUB_0.1.2_x64-setup.exe`.
  Two existing AppBar release warnings remain; that code was not changed.
- Updated web directory deployed to the existing project's production branch:
  https://62cdd02d.mona-hub.pages.dev . The production hostname serves the new
  `/auth/mona-session.js`; unauthenticated `/app/app.js` returns 302 as expected.
- Runtime DB inspection found one historical verification row and the expected
  active tenant; no runtime JIT test row was inserted and no DB reset was run.
- No Git add/commit/push was performed. The pre-existing capability edit and
  AC/DC bridge-token work were preserved.
- Still pending: correct Zero Trust IdP claim configuration and fresh interactive
  Entra A/B login using the updated native binary. Automated fixtures are not
  evidence that this live step has succeeded.
