import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const styles = await readFile(new URL("../web/login/login.css", import.meta.url), "utf8");

test("Cloudflare Access progress fills once from zero to completion", () => {
  assert.match(styles, /\.login-progress-bar\s*\{[\s\S]*?width:\s*0/);
  assert.match(styles, /animation:\s*login-progress-fill\s+1\.2s\s+ease-out\s+forwards/);
  assert.match(styles, /@keyframes\s+login-progress-fill\s*\{[\s\S]*?from\s*\{\s*width:\s*0/);
  assert.match(styles, /@keyframes\s+login-progress-fill\s*\{[\s\S]*?to\s*\{\s*width:\s*100%/);
  assert.doesNotMatch(styles, /animation:\s*login-progress-fill[^;]*infinite/);
});
