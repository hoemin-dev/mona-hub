# MonaHub popup compatibility harness (temporary / local only)

No production data, libraries, server integration, native IPC grants, or deployment.
Run from the repository root in a separate terminal:

```powershell
py -m http.server 8088 --bind 127.0.0.1 --directory web/popup-test
```

Open http://127.0.0.1:8088/ for an ordinary-browser baseline. Stop with Ctrl+C.
Binding explicitly to loopback avoids exposing this fixture to the LAN. Use only
synthetic files and messages. Python may log request paths: do not enter secrets in URLs.

## AppBar execution and the remote-source constraint

Build/run the locally changed Rust application using `npm run tauri:dev:access`.
Authenticate normally. The trusted main caller must still be exactly the MonaHub
origin and `/app/` path with an authenticated Rust session.

**The AppBar loads remote JS, not the edited local `web/app/app.js`.** This change
does not deploy that file or make a local `/app/` caller trusted. For local-only
AppBar testing, use WebView2 DevTools **Local Overrides** in the trusted main window:
override the remote `/app/app.js` response with the contents of local `web/app/app.js`,
then reload. If Local Overrides are unavailable in that WebView2 runtime, the
actual AppBar click test remains pending; do not change auth/capabilities to bypass it.
Remove the override after testing. No Cloudflare/Entra change is needed.

For command-path-only diagnosis, this explicit call can be executed in the
authenticated trusted main window's DevTools console (not in the harness window):

```js
await window.__TAURI__.core.invoke("open_web_app", {
  id: "popup-test", url: "http://127.0.0.1:8088/"
});
```

That console call is **not** a substitute for recording an actual AppBar click.

Call chain when the edited app.js is loaded:

`mockApps → Test button click → openWebApp(app) → invoke(open_web_app)`
`→ trusted caller + exact id/URL + authenticated state validation`
`→ shared serialized lookup/build → webapp-popup-test → restore/show/focus`.

Closing the root with the window X and clicking Test again should create a new
root window; clicking while it is minimized should restore the existing one.
PDFYS uses the same original builder options and does not receive the test handlers.
The harness is not integrated into logout cleanup; close test windows manually.

## Popup implementation and policy

`src-tauri/src/popup_test.rs::configure` attaches `on_new_window` to the test root
and every popup. An allowed request builds a uniquely labeled
`popup-test-popup-N` WebviewWindow with **about:blank**, calls
`.window_features(features)` and `.disable_drag_drop_handler()`, recursively
configures it, and returns **`NewWindowResponse::Create { window }`**. It does not
manually navigate an unrelated window to the requested URL. On Windows, the
window features also carry the opener's WebView2 environment.

Parsed URL policy (also enforced on subsequent navigation/redirects):

- HTTP, host `127.0.0.1`, effective port `8088`, any path/query/fragment.
- Exactly `about:blank`.
- Exactly the parsed HTTPS URL `https://example.com/`, no query/fragment/other path.
- No credentials. Everything else denied, including file/javascript/data/custom
  schemes, localhost, other ports, other private addresses and arbitrary external sites.

URL parsing normalizes equivalent spellings; the **launch command** separately
compares the raw id/URL strings and permits only `popup-test` +
`http://127.0.0.1:8088/`. PDFYS's exact pair is unchanged.

The real target=_blank anchor is not intercepted or rewritten; its implicit
noopener behavior is part of the test. A null opener there is not automatically
a bug. Repeat named-popup while open to check reuse and after closing to check
recreation. Every child has the same controls for nested popup tests.

example.com provides cross-origin navigation and DOM isolation testing, not a
cooperating postMessage endpoint. ACK timeout against it is expected. Use the
same-origin child for bidirectional postMessage/ACK. About:blank has no harness
script; use its parent's reference inspection or DevTools.

## Manual runtime checklist

1. Record OS, WebView2 version, userAgent, and ordinary-browser baseline.
2. Open each popup variant; record returned reference, opener, rendered page,
   popup dimensions, and Rust parent/popup labels + Create/Deny decision.
3. Test both message directions. Only a received message/ACK earns PASS; sending
   alone is WAIT. The receiver validates channel, source, origin and message type.
4. Open a nested popup and compare its parent label in Rust. Try the denied-port
   button: Rust Deny/no new window is the authoritative result, not just JS return.
5. Test file input and both OS-file drop and internal text drag in root and child.
   Native drag/drop handling is disabled for both so HTML5 receives the events.
   No file is uploaded or read. Logs omit names/content.
6. Test static `sample.txt` and Blob downloads. A click is only WAIT; inspect the
   saved fixture and Rust `download finished success=true`. Blob URLs are download
   resources, not an exception allowing blob popup/navigation URLs.
7. Click child **window.close()**, then test window X separately. Record:
   - screen `close requested`, opener `pagehide` if emitted, and `popup.closed=true`;
   - Rust diagnostic `close-requested`, Tauri `CloseRequested` if emitted;
   - Rust `popup destroyed` and `registry_removed=true` on **WindowEvent::Destroyed**;
   - after two seconds, `close-observation`: Tauri-managed window presence and registry.
8. If the content vanishes but the outer window remains, record it as such; close
   the remaining window manually and check Destroyed/registry cleanup. Do not call
   it a successful window.close merely because a JS reference says closed.

**Observation limitation:** Tauri's public WebviewEvent in the locked version
exposes drag/drop, not native WebView close. Wry 0.55.1's Windows
WindowCloseRequested handler calls DestroyWindow on its WebView host. This harness
does not hook native events or add a workaround. `native_webview_closed=UNOBSERVABLE`
is intentional: Tauri registry presence is not proof the native WebView is alive,
and pagehide can indicate navigation, not destruction. A direct native-close trace
would require a later, separately scoped runtime instrumentation decision.

Rust observes a fixed set of **untrusted diagnostic-only document.title markers**
for page events; this grants no native commands. Rapid title events may coalesce,
so retain the on-screen log too. Close uses a short delay to allow its marker to
reach the host before invoking the unchanged window.close API. Cross-origin pages
do not run these markers. Popup registry entries are removed only on real Tauri
Destroyed, never merely on pagehide/JS close signals. Native window title remains
the test title; page titles are not copied into native UI.

## Permissions and scope

No capability file changes. Existing capabilities enumerate only main, login,
profile-popup; neither `webapp-popup-test` nor `popup-test-popup-*` matches them.
The harness uses no invoke, event plugin, filesystem plugin or other native IPC.
Global Tauri internals being present is not a grant of capability. Ordinary DOM
file selection/download remain browser features. Parent labels and popup labels
appear in Rust; page logs use local child-reference IDs (not native labels).
Queries/fragments/credentials, arbitrary paths, message payloads and file names
are not copied to diagnostic logs.

## Checks

```powershell
cd src-tauri
cargo check --locked
cargo test --locked
cd ..
node --check web/app/app.js
node --check web/popup-test/app.js
npm run check
git diff --check
```

Unit/static checks and HTTP 200 do not verify actual Tauri GUI popup behavior.
No git add/commit/push is part of this harness workflow.
