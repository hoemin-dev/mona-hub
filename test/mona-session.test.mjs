import test from "node:test";
import assert from "node:assert/strict";
import { MonaSession } from "../web/auth/mona-session.js";
const identity = { tenant_id: "TEN-TEST", person_id: "PER-ABCDEFGH", status: "ACTIVE" };
test("duplicates share one request and expose only Mona identifiers", async () => {
  let calls = 0;
  const session = new MonaSession(async () => { calls++; return identity; });
  const [a, b] = await Promise.all([session.resolve(), session.resolve()]);
  assert.equal(calls, 1); assert.equal(a, b);
  assert.deepEqual(a, { tenant_id: identity.tenant_id, person_id: identity.person_id });
  await session.resolve(); assert.equal(calls, 1);
});
test("offline can retry; stale result cannot restore identity after logout", async () => {
  let finish;
  const session = new MonaSession(() => new Promise(resolve => { finish = resolve; }));
  const pending = session.resolve(); await Promise.resolve(); session.clear();
  finish(identity); assert.equal(await pending, null); assert.equal(session.state.identity, null);
  let fail = true;
  const retry = new MonaSession(async () => { if (fail) throw "bridge-offline"; return identity; });
  await retry.resolve(); assert.equal(retry.state.error, "bridge-offline");
  fail = false; await retry.resolve(); assert.equal(retry.state.status, "ready");
});
test("invalid response and unexpected errors cannot leak to session state", async () => {
  const session = new MonaSession(async () => { throw new Error("private details"); });
  await session.resolve(); assert.equal(session.state.error, "identity-resolve-failed");
  const invalid = new MonaSession(async () => ({ ...identity, person_id: "unsafe" }));
  await invalid.resolve(); assert.equal(invalid.state.error, "bridge-response-invalid");
});
