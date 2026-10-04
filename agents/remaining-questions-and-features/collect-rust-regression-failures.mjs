import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { runBoundedCommand } from "../../scripts/run-with-deadline.mjs";
import { parseRustTestCases } from "../../scripts/run-full-regression.mjs";

const root = process.cwd();
const inventory = JSON.parse(readFileSync(path.join(root, ".tmp/full-regression/artifact-inventory.json"), "utf8"));
const started = Date.now();
const failures = [];
const cases = [];
let next = 0;
let passed = 0;
const output = path.join(root, ".tmp/main-regression-2026-10-05/rust-failure-collection.json");
const remaining = () => Math.max(1, Math.min(10000, started + 299000 - Date.now()));
console.log(JSON.stringify({ purpose: "既有预构建产物的失败定位；来源 digest 不证明并发产物不可变，不替代正式回归；须由进程外300000ms监督启动", source_digest: inventory.source_fingerprint.digest, concurrency: 64, rayon_threads: 1, deadline_ms: 300000 }));

async function worker(items, concurrency, callback) {
  next = 0;
  await Promise.all(Array.from({ length: concurrency }, async () => {
    for (;;) {
      const index = next++;
      if (index >= items.length || Date.now() >= started + 299000) return;
      await callback(items[index]);
    }
  }));
}

await worker(inventory.artifacts, 8, async artifact => {
  const executable = path.join(root, artifact.executable_relative_path);
  try {
    const all = await runBoundedCommand({ command: executable, args: ["--list", "--format=terse"], timeoutMs: remaining(), captureOutput: true });
    const ignored = await runBoundedCommand({ command: executable, args: ["--list", "--format=terse", "--ignored"], timeoutMs: remaining(), captureOutput: true });
    const omitted = new Set(parseRustTestCases(ignored.stdout));
    for (const name of parseRustTestCases(all.stdout)) {
      if (!omitted.has(name)) cases.push({ executable, label: artifact.label, name });
    }
  } catch (error) {
    failures.push({ label: artifact.label, stage: "listing", error: error.message });
  }
});

await worker(cases, 64, async testCase => {
  try {
    await runBoundedCommand({ command: testCase.executable, args: [testCase.name, "--exact", "--test-threads=1"], env: { ...process.env, RAYON_NUM_THREADS: "1" }, timeoutMs: remaining(), captureOutput: true,
      onStdout: chunk => writeFileSync(path.join(root, ".tmp/main-regression-2026-10-05/rust-case-output.log"), `${testCase.label} ${testCase.name}\n${chunk}`, { flag: "a" }),
      onStderr: chunk => writeFileSync(path.join(root, ".tmp/main-regression-2026-10-05/rust-case-output.log"), `${testCase.label} ${testCase.name}\n${chunk}`, { flag: "a" }),
    });
    passed += 1;
  } catch (error) {
    const failure = { label: testCase.label, name: testCase.name, error: error.message };
    failures.push(failure);
    console.log(JSON.stringify(failure));
  }
});

const result = { source_digest: inventory.source_fingerprint.digest, total: cases.length, passed, failures, completed: passed + failures.filter(failure => failure.stage !== "listing").length === cases.length, wall_ms: Date.now() - started };
writeFileSync(output, JSON.stringify(result, null, 2));
console.log(JSON.stringify(result));
if (failures.length > 0 || !result.completed) process.exitCode = 1;
