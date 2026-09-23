import assert from "node:assert/strict";
import { chmodSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { createHash } from "node:crypto";
import { readdirSync, statSync, writeFileSync, mkdirSync } from "node:fs";
import test from "node:test";

const script = resolve("scripts/desktop/wayland-native-qa.sh");

function snapshotDirectory(directory) {
  const entries = [];
  const visit = (current, relative) => {
    const stat = statSync(current, { bigint: true });
    entries.push({ relative, kind: stat.isDirectory() ? "directory" : "file", mtime: stat.mtimeNs.toString() });
    if (stat.isDirectory()) {
      for (const name of readdirSync(current).sort()) visit(join(current, name), join(relative, name));
      return;
    }
    entries.push({
      relative,
      digest: createHash("sha256").update(readFileSync(current)).digest("hex"),
    });
  };
  visit(directory, ".");
  return entries;
}

function failedHarness(environment) {
  try {
    execFileSync("bash", [script], { encoding: "utf8", env: environment, stdio: "pipe" });
    assert.fail("expected the harness to fail");
  } catch (error) {
    return error;
  }
}

test("Wayland QA harness keeps a bounded 2x2 probe and a Wayland-only final route", () => {
  const output = execFileSync("bash", ["-n", script], { encoding: "utf8" });
  assert.equal(output, "");
  const source = readFileSync(script, "utf8");
  assert.match(source, /wayland-probe\.sh/);
  assert.match(source, /for renderer in pixman default/);
  assert.match(source, /for dimensions in explicit default/);
  assert.match(source, /renderer_evidence/);
  assert.match(source, /advertised_mode/);
  assert.match(source, /--expect unsupported/);
  assert.match(source, /--expect supported/);
  assert.match(source, /cargo build -p stock-market-game --features simulation-diagnostics/);
  assert.match(source, /task-3-wayland\.actor-default\.txt/);
  assert.match(source, /task-3-wayland\.actor-release\.txt/);
  assert.match(source, /task-3-wayland\.actor-feature\.txt/);
  assert.match(source, /task-3-wayland\.engine-bounded\.txt/);
  assert.match(source, /task-3-wayland\.capture-attempts\.tsv/);
  assert.match(source, /timeout 20s weston-screenshooter/);
  assert.match(source, /GDK_BACKEND=wayland/);
  assert.match(source, /env -u DBUS_SESSION_BUS_ADDRESS/);
  assert.match(source, /rm -rf "\$runtime_dir" "\$config_dir" "\$capture_dir"/);
  assert.match(source, /task-3-wayland\.run-\$RUN_ID/);
  assert.doesNotMatch(source, /clear_previous_evidence/);
  assert.match(source, /task-3-wayland\.environment-default\.txt/);
  assert.match(source, /task-3-wayland\.worktree-status\.txt/);
  assert.doesNotMatch(source, /xvfb-run/);
  const probe = resolve("scripts/desktop/wayland-probe.sh");
  assert.equal(execFileSync("bash", ["-n", probe], { encoding: "utf8" }), "");
  const probeSource = readFileSync(probe, "utf8");
  assert.match(probeSource, /screenshooter_exit/);
  assert.match(probeSource, /renderer_evidence/);
  assert.match(probeSource, /advertised_mode/);
  assert.match(probeSource, /stop_probe\n\nif rg -q 'Using Pixman renderer'/);
});

test("Wayland QA harness rejects a missing native binary before it starts Weston", () => {
  const directory = mkdtempSync(join(tmpdir(), "wayland-native-qa-test-"));
  const evidence = join(directory, "evidence");
  const missingBinary = join(directory, "missing-stock-market-game");
  try {
    const result = execFileSync("bash", [script], {
      encoding: "utf8",
      env: { ...process.env, WAYLAND_APP_BINARY: missingBinary, WAYLAND_EVIDENCE_DIR: evidence },
      stdio: "pipe",
    });
    assert.fail(`expected failure, received ${result}`);
  } catch (error) {
    assert.equal(error.status, 1);
    assert.match(error.stderr, /missing desktop binary/);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("Wayland QA harness rejects an existing explicit evidence directory before changing it", () => {
  const root = mkdtempSync(join(tmpdir(), "wayland-native-qa-explicit-"));
  const evidence = join(root, "existing-evidence");
  const missingBinary = join(root, "missing-stock-market-game");
  mkdirSync(evidence);
  writeFileSync(join(evidence, "sentinel.txt"), "keep this evidence immutable\n");
  const before = snapshotDirectory(evidence);
  try {
    const error = failedHarness({ ...process.env, WAYLAND_APP_BINARY: missingBinary, WAYLAND_EVIDENCE_DIR: evidence });
    assert.deepEqual(snapshotDirectory(evidence), before);
    assert.match(error.stderr, /refusing to overwrite existing evidence directory/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Wayland QA harness rejects a duplicate caller-specified run ID before changing it", () => {
  const root = mkdtempSync(join(tmpdir(), "wayland-native-qa-run-id-"));
  const runId = "duplicate";
  const evidence = join(root, `task-3-wayland.run-${runId}`);
  const missingBinary = join(root, "missing-stock-market-game");
  mkdirSync(evidence);
  writeFileSync(join(evidence, "sentinel.txt"), "do not replace this run\n");
  const before = snapshotDirectory(evidence);
  try {
    const error = failedHarness({
      ...process.env,
      WAYLAND_APP_BINARY: missingBinary,
      WAYLAND_EVIDENCE_ROOT: root,
      WAYLAND_RUN_ID: runId,
    });
    assert.deepEqual(snapshotDirectory(evidence), before);
    assert.match(error.stderr, /refusing to overwrite existing evidence directory/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Wayland QA harness adds a deterministic suffix when its automatic timestamp candidate exists", () => {
  const root = mkdtempSync(join(tmpdir(), "wayland-native-qa-auto-id-"));
  const binDirectory = join(root, "bin");
  const evidenceRoot = join(root, "evidence-root");
  const fixedTimestamp = "20300101-000000";
  const occupiedRun = join(evidenceRoot, `task-3-wayland.run-${fixedTimestamp}`);
  const missingBinary = join(root, "missing-stock-market-game");
  const environment = { ...process.env, WAYLAND_APP_BINARY: missingBinary, WAYLAND_EVIDENCE_ROOT: evidenceRoot };
  delete environment.WAYLAND_EVIDENCE_DIR;
  delete environment.WAYLAND_RUN_ID;
  mkdirSync(binDirectory);
  writeFileSync(join(binDirectory, "date"), `#!/usr/bin/env bash\nprintf '%s\\n' '${fixedTimestamp}'\n`);
  chmodSync(join(binDirectory, "date"), 0o755);
  mkdirSync(evidenceRoot);
  mkdirSync(occupiedRun);
  writeFileSync(join(occupiedRun, "sentinel.txt"), "preserve occupied automatic run\n");
  const before = snapshotDirectory(occupiedRun);
  try {
    const error = failedHarness({ ...environment, PATH: `${binDirectory}:${environment.PATH}` });
    const suffixedRun = join(evidenceRoot, `task-3-wayland.run-${fixedTimestamp}-1`);
    assert.deepEqual(snapshotDirectory(occupiedRun), before);
    assert.match(error.stderr, /missing desktop binary/);
    assert.equal(readFileSync(join(suffixedRun, "task-3-wayland.run.txt"), "utf8"), `run_id=${fixedTimestamp}-1\nevidence_dir=${suffixedRun}\n`);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
