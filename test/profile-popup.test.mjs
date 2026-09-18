import assert from "node:assert/strict";
import test from "node:test";

class FakeElement extends EventTarget {
  hidden = false;
  disabled = false;

  focus() {}
}

test("profile popup clears the retained confirmation frame when it is hidden", async () => {
  const elements = {
    menuView: new FakeElement(),
    confirmView: new FakeElement(),
    logoutItem: new FakeElement(),
    cancelButton: new FakeElement(),
    confirmButton: new FakeElement()
  };
  elements.confirmView.hidden = true;
  const placeholder = new FakeElement();
  const commands = [];
  const stateAtInvoke = [];
  const windowTarget = new EventTarget();
  windowTarget.location = {
    href: "https://tauri.localhost/profile-popup/",
    origin: "https://tauri.localhost",
    assign() {}
  };
  windowTarget.__TAURI__ = {
    core: {
      invoke: async command => {
        commands.push(command);
        stateAtInvoke.push({
          menuHidden: elements.menuView.hidden,
          confirmHidden: elements.confirmView.hidden
        });
      }
    }
  };

  globalThis.window = windowTarget;
  globalThis.document = {
    getElementById: id => elements[id],
    querySelectorAll: selector => selector === "[data-placeholder]" ? [placeholder] : [placeholder, elements.logoutItem]
  };

  await import(`../web/profile-popup/popup.js?test=${Date.now()}`);

  elements.logoutItem.dispatchEvent(new Event("click"));
  assert.equal(elements.menuView.hidden, true);
  assert.equal(elements.confirmView.hidden, false);

  windowTarget.dispatchEvent(new Event("blur"));
  assert.equal(elements.menuView.hidden, false);
  assert.equal(elements.confirmView.hidden, true);

  elements.logoutItem.dispatchEvent(new Event("click"));
  elements.confirmButton.dispatchEvent(new Event("click"));
  await new Promise(resolve => setImmediate(resolve));

  assert.deepEqual(commands, ["confirm_access_logout"]);
  assert.deepEqual(stateAtInvoke, [{ menuHidden: false, confirmHidden: true }]);
  assert.equal(elements.menuView.hidden, false);
  assert.equal(elements.confirmView.hidden, true);
  assert.equal(elements.confirmButton.disabled, false);

  delete globalThis.window;
  delete globalThis.document;
});
