import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

function job(workflow, name) {
  const section = workflow.split(new RegExp(`^  ${name}:`, "m"))[1];
  assert.ok(section, `workflow must define ${name}`);
  return section.split(/^  [a-z][a-z-]*:/m)[0];
}

const shortCommand = "node scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-concurrency=4 scripts/prune-actions-cache.test.mjs scripts/cache-workflow.test.mjs";
const cleanupCommand = "node scripts/run-long-validation.mjs 300000 -- node scripts/prune-actions-cache.mjs --repo ${{ github.repository }} --apply";

function assertRunCommands(cleanup) {
  const commands = [...cleanup.matchAll(/^        run: (.+)$/gm)].map((match) => match[1]);
  assert.deepEqual(commands, [shortCommand, cleanupCommand]);
}

for (const [filename, dependencies] of [["ci", "build"], ["distributions", "[frontend, native, server]"]]) {
  const workflow = () => readFileSync(new URL(`../.github/workflows/${filename}.yml`, import.meta.url), "utf8");

  test(`${filename}: cleanup waits for all cache-writing jobs, including failed builds`, () => {
    const cleanup = job(workflow(), "prune-caches");
    assert.ok(cleanup.includes(`needs: ${dependencies}\n`));
    assert.match(cleanup, /if: always\(\) && !cancelled\(\) && github\.event_name != 'pull_request'/);
    assert.doesNotMatch(cleanup, /needs\.[a-z-]+\.result == 'success'|continue-on-error:/);
    assert.match(cleanup, /runs-on: ubuntu-24\.04/);
  });

  test(`${filename}: cleanup has job-scoped deletion rights, not PR or build-job rights`, () => {
    const text = workflow();
    const cleanup = job(text, "prune-caches");
    assert.match(cleanup, /permissions:\s+contents: read\s+actions: write/);
    assert.match(cleanup, /GH_TOKEN: \$\{\{ github\.token \}\}/);
    assert.match(cleanup, /persist-credentials: false/);
    assert.doesNotMatch(cleanup, /secrets\.|contents: write|pull_request_target/);
    const otherJobs = text.replace(`  prune-caches:${cleanup}`, "");
    assert.doesNotMatch(otherJobs, /actions: write|pull_request_target/);
  });

  test(`${filename}: cleanup is repository-wide serialized and uses bounded parallel short tests`, () => {
    const cleanup = job(workflow(), "prune-caches");
    assert.match(cleanup, /concurrency:\s+group: actions-cache-prune\s+cancel-in-progress: false/);
    assert.match(cleanup, /node-version: 24\.18\.0/);
    assertRunCommands(cleanup);
    assert.doesNotMatch(cleanup, /run-full-regression|cargo build|pnpm install|cache\/save|upload-artifact|cache delete --all/);
  });
}

test("bounded-command contracts reject comment-only lookalikes and disabled file isolation", () => {
  const lines = [`        run: ${shortCommand}`, `        run: ${cleanupCommand}`];
  assertRunCommands(lines.join("\n"));
  for (const index of [0, 1]) {
    const commented = [...lines];
    commented[index] = `        # run: ${index === 0 ? shortCommand : cleanupCommand}`;
    assert.throws(() => assertRunCommands(commented.join("\n")), /AssertionError/);
  }
  assert.throws(() => assertRunCommands(lines.join("\n").replace("--test-concurrency=4", "--test-concurrency=4 --test-isolation=none")), /AssertionError/);
});
