import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import { readFile } from 'node:fs/promises';
import { MonaSession } from '../web/auth/mona-session.js';
const loadingSource = (await readFile(new URL('../web/auth/session-loading.js', import.meta.url), 'utf8')).replace('export function', 'function');
const appSource = (await readFile(new URL('../web/app/app.js', import.meta.url), 'utf8')).replace(/^import .*;\r?$/gm, '');
function fixture({ label = 'main', snapshot, authState } = {}) {
  const elements = new Map();
  const classes = new Set();
  const node = () => ({ dataset: {}, setAttribute() {}, addEventListener() {}, append() {}, querySelectorAll: () => [], getBoundingClientRect: () => ({ toJSON: () => ({}) }), remove() { elements.delete(this.id); } });
  const main = node();
  const document = { documentElement: { classList: { toggle: (key, value) => value ? classes.add(key) : classes.delete(key) } }, querySelector: () => main, getElementById: key => elements.get(key), createElement: node, body: { append: element => elements.set(element.id, element) } };
  elements.set('appList', node()); elements.set('profileButton', node());
  let finish;
  const pending = new Promise(resolve => { finish = resolve; });
  const calls = [];
  const window = new EventTarget();
  window.__monaSessionLoading = snapshot;
  window.__monaAuthState = authState;
  window.location = { href: 'https://mona-hub.pages.dev/app/', replace: () => calls.push('redirect') };
  const update = loading => { window.__monaSessionLoading = loading; window.dispatchEvent(new Event('mona:session-loading')); };
  window.__TAURI__ = { window: { getCurrentWindow: () => ({ label }) }, core: { invoke: async (command, args) => {
    calls.push({ command, args });
    if (command === 'sync_acdc_identity') return pending;
    if (command === 'session_ui_ready' && window.__monaAuthState !== 'logout-pending') update(false);
  } } };
  const context = vm.createContext({ window, document, console: { info() {}, error() {} }, Event, CustomEvent, MonaSession, AccessAuthProvider: class {}, AuthController: class {}, authConfig: {}, getComputedStyle: () => ({ pointerEvents: 'auto' }) });
  vm.runInContext(loadingSource, context);
  return { context, classes, elements, main, calls, finish, update };
}
const settle = () => new Promise(resolve => setImmediate(resolve));
for (const path of ['manual', 'fast path']) test(`${path}: loading lasts until cached PER session resolves`, async () => {
  const f = fixture({ snapshot: path === 'manual' ? false : true });
  f.update(true);
  vm.runInContext(appSource, f.context);
  await settle();
  assert.equal(f.main.inert, true);
  assert.ok(f.elements.has('sessionLoading'));
  assert.equal(f.calls.length, 1);
  f.finish({ person_id: 'PER-ABCDEFGH', tenant_id: 'TEN-TEST', status: 'ACTIVE' });
  await settle();
  assert.equal(f.calls.filter(c => c.command === 'sync_acdc_identity').length, 1);
  assert.equal(f.calls[1].args.ready, true);
  assert.equal(f.main.inert, false);
  assert.equal(f.elements.has('sessionLoading'), false);
});
test('native authentication/API failure removes loading and restores login controls', () => {
  const f = fixture({ snapshot: true });
  f.update(false);
  assert.equal(f.main.inert, false);
  assert.equal(f.classes.has('session-loading'), false);
});
test('invalid cached PER signals failure to native, never readiness', async () => {
  const f = fixture(); vm.runInContext(appSource, f.context);
  f.finish({ person_id: 'invalid' }); await settle();
  assert.equal(f.calls[1].command, 'session_ui_ready');
  assert.equal(f.calls[1].args.ready, false);
});
test('hidden login WebView never acknowledges or redirects the main session', async () => {
  const f = fixture({ label: 'login' }); vm.runInContext(appSource, f.context);
  f.finish(null); await settle();
  assert.equal(f.calls.length, 1);
});
test('snapshot before module load restores idle instead of leaving a stuck spinner', () => {
  const f = fixture({ snapshot: false });
  assert.equal(f.main.inert, false);
  assert.equal(f.elements.has('sessionLoading'), false);
});
test('precommit logout preserves the rendered AppBar without a loading overlay', async () => {
  const f = fixture({ snapshot: false, authState: 'logout-pending' });
  vm.runInContext(appSource, f.context);
  f.finish(null);
  await settle();
  assert.equal(f.main.inert, false);
  assert.equal(f.classes.has('session-loading'), false);
  assert.equal(f.elements.has('sessionLoading'), false);
  assert.equal(f.calls.includes('redirect'), false);

  f.update(false);
  assert.equal(f.main.inert, false);
  assert.equal(f.elements.has('sessionLoading'), false);
});


