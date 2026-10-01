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
  for (const command of ["wasm-pack build", "cargo clippy --workspace", "cargo build -p server --release", "pnpm --filter web test:e2e"]) {
    assert.match(steps[stepContaining(`run: node scripts/run-long-validation.mjs 300000 -- ${command}`)],
      /run-long-validation\.mjs 300000 --/);
  }
});
