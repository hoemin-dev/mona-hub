import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const styles = await readFile(new URL("../web/login/login.css", import.meta.url), "utf8");

test("Cloudflare Access progress starts outside the track and remains indeterminate", () => {
  assert.match(styles, /\.login-progress-bar\s*\{[\s\S]*?width:\s*32%/);
  assert.match(styles, /\.login-progress-bar\s*\{[\s\S]*?transform:\s*translateX\(-100%\)/);
  assert.match(styles, /@keyframes\s+login-progress-slide\s*\{[\s\S]*?translateX\(312\.5%\)/);
});

test("reduced motion uses a partial static indicator instead of a completed bar", () => {
  const reducedMotion = styles.match(
    /@media\s*\(prefers-reduced-motion:\s*reduce\)\s*\{([\s\S]*?)\n\}/
  )?.[1] ?? "";

  assert.match(reducedMotion, /width:\s*28%/);
  assert.match(reducedMotion, /transform:\s*none/);
  assert.doesNotMatch(reducedMotion, /width:\s*100%/);
});
