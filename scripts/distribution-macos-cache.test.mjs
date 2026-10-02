import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

test("macOS library precompile uses the same deployment target as the pinned Tauri bundler", () => {
  const workflow = readFileSync(new URL("../.github/workflows/distributions.yml", import.meta.url), "utf8");
  const prepare = workflow.split("- name: Prepare desktop native compile cache")[1]?.split(/^      - name:/m)[0];
  assert.ok(prepare);
  assert.match(prepare, /MACOSX_DEPLOYMENT_TARGET: \$\{\{ runner\.os == 'macOS' && '10\.13' \|\| '' \}\}/);
  const config = JSON.parse(readFileSync(new URL("../apps/desktop/src-tauri/tauri.conf.json", import.meta.url), "utf8"));
  const macos = config.bundle.macOS;
  if (macos !== undefined && macos.minimumSystemVersion !== undefined) {
    assert.equal(macos.minimumSystemVersion, "10.13", "deployment configuration changes must also update the precompile target");
  }
});
