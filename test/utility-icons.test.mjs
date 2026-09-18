import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

for (const view of ["app", "prelogin"]) {
  test(`${view} shows toolbox and help above the profile`, async () => {
    const html = await readFile(new URL(`../web/${view}/index.html`, import.meta.url), "utf8");
    const toolbox = html.indexOf('aria-label="공구박스"');
    const help = html.indexOf('aria-label="도움말"');
    const profile = html.indexOf('class="profile-area"');

    assert.ok(toolbox >= 0);
    assert.ok(help > toolbox);
    assert.ok(profile > help);
  });
}
