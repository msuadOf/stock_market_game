import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { it } from "node:test";

const workflow = readFileSync(new URL("../.github/workflows/ci.yml", import.meta.url), "utf8");
const steps = workflow.split(/^      - name: /m).slice(1);

function stepContaining(command) {
  const index = steps.findIndex((step) => step.includes(command));
  assert.notEqual(index, -1, `CI must run ${command}`);
  return index;
}

it("checks generated bindings after the Rust export tests have actually run", () => {
  assert.ok(stepContaining("scripts/check-generated-types.mjs")
    > stepContaining("scripts/run-full-regression.mjs execute"));
});

it("omits costly native debug symbols without disabling assertions or tests", () => {
  assert.match(workflow, /^  CARGO_PROFILE_DEV_DEBUG: "0"$/m);
  assert.match(workflow, /^  CARGO_PROFILE_TEST_DEBUG: "0"$/m);
  assert.doesNotMatch(workflow, /CARGO_PROFILE_\w+_(?:DEBUG_ASSERTIONS|OVERFLOW_CHECKS):\s*(?:false|"false")/);
  assert.ok(stepContaining("scripts/run-full-regression.mjs build")
    < stepContaining("scripts/run-full-regression.mjs execute"));
});

it("gives native and WASM builds an explicit CPU budget and external deadline", () => {
  assert.match(steps[stepContaining("availableParallelism")], /GITHUB_ENV/);
  assert.match(steps[stepContaining("availableParallelism")], /CARGO_BUILD_JOBS/);
  assert.match(steps[stepContaining("run: node scripts/frontend-build.mjs")], /--jobs \$\{\{ env.CARGO_BUILD_JOBS \}\}/);
  assert.match(steps[stepContaining("run: bash scripts/build.sh server")], /--jobs "\$CARGO_BUILD_JOBS"/);
  assert.match(steps[stepContaining("run: scripts\\build.bat server")], /--jobs %CARGO_BUILD_JOBS%/);
  for (const command of ["cargo clippy --workspace", "pnpm --filter web test:e2e"]) {
    assert.match(steps[stepContaining(`run: node scripts/run-long-validation.mjs 300000 -- ${command}`)],
      /run-long-validation\.mjs 300000 --/);
  }
});

it("reuses same-platform Cargo artifacts when dependency manifests change", () => {
  const restore = steps[stepContaining("Restore sealed Cargo cache")];
  assert.match(restore, /restore-keys:[\s\S]*sealed-cargo-v2-no-debug-\$\{\{ runner\.os \}\}-\$\{\{ runner\.arch \}\}-\s*$/);
});

it("pins wasm-pack after Cargo cache restoration and verifies the installed version", () => {
  const install = stepContaining("uses: jetli/wasm-pack-action@v0.4.0");
  assert.match(steps[install], /version: v0\.13\.1/);
  assert.ok(install > stepContaining("uses: Swatinem/rust-cache@v2"));
  const check = stepContaining("Expected wasm-pack 0.13.1");
  assert.ok(check > install && check < stepContaining("run: node scripts/frontend-build.mjs"));
});
