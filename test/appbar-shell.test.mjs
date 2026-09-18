import test from "node:test";
import assert from "node:assert/strict";
import vm from "node:vm";
import { readFile } from "node:fs/promises";

const shellSource = await readFile(
  new URL("../web/appbar-shell.js", import.meta.url),
  "utf8"
);

function loadShell(label = "main") {
  const listeners = new Map();
  const document = {
    addEventListener(type, listener, options) {
      listeners.set(type, { listener, options });
    }
  };
  const window = {
    __TAURI__: { window: { getCurrentWindow: () => ({ label }) } }
  };
  vm.runInContext(shellSource, vm.createContext({ document, window, Set }));
  return listeners;
}

function fakeEvent(overrides = {}) {
  return {
    key: "",
    ctrlKey: false,
    prevented: false,
    stopped: false,
    preventDefault() { this.prevented = true; },
    stopImmediatePropagation() { this.stopped = true; },
    ...overrides
  };
}

test("AppBar suppresses its context menu in the capture phase", () => {
  const listeners = loadShell();
  const entry = listeners.get("contextmenu");
  const event = fakeEvent();

  assert.equal(entry.options.capture, true);
  entry.listener(event);
  assert.equal(event.prevented, true);
  assert.equal(event.stopped, true);
});

for (const shortcut of [
  { key: "F5" },
  { key: "r", ctrlKey: true },
  { key: "R", ctrlKey: true, shiftKey: true },
  { key: "p", ctrlKey: true },
  { key: "s", ctrlKey: true }
]) {
  test(`AppBar suppresses browser shortcut ${JSON.stringify(shortcut)}`, () => {
    const event = fakeEvent(shortcut);
    loadShell().get("keydown").listener(event);
    assert.equal(event.prevented, true);
    assert.equal(event.stopped, true);
  });
}

test("AppBar leaves non-browser keyboard input alone", () => {
  const event = fakeEvent({ key: "c", ctrlKey: true });
  loadShell().get("keydown").listener(event);
  assert.equal(event.prevented, false);
  assert.equal(event.stopped, false);
});

test("only AppBar documents import the shell guard", async () => {
  const [app, prelogin, login] = await Promise.all([
    readFile(new URL("../web/app/app.js", import.meta.url), "utf8"),
    readFile(new URL("../web/prelogin/prelogin.js", import.meta.url), "utf8"),
    readFile(new URL("../web/login/login.js", import.meta.url), "utf8")
  ]);

  assert.match(app, /import "\.\.\/appbar-shell\.js"/);
  assert.match(prelogin, /import "\.\.\/appbar-shell\.js"/);
  assert.doesNotMatch(login, /appbar-shell/);
});

test("the login WebView is not guarded even if it visits an AppBar document", () => {
  const listeners = loadShell("login");
  assert.equal(listeners.size, 0);
});
