import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";

const script = resolve("scripts/desktop/wayland-evidence-integrity.mjs");
const artifactNames = [
  "task-3-wayland.run.txt",
  "task-3-wayland.worktree-status.txt",
  "task-3-wayland.command.txt",
  "task-3-wayland.probe-matrix.tsv",
  "task-3-wayland.weston.log",
  "task-3-wayland.wayland-info.txt",
  "task-3-wayland.actor-default.txt",
  "task-3-wayland.actor-release.txt",
  "task-3-wayland.actor-feature.txt",
  "task-3-wayland.engine-bounded.txt",
  "task-3-wayland.environment-default.txt",
  "task-3-wayland.environment-feature.txt",
  "task-3-wayland.tauri-default.log",
  "task-3-wayland.tauri-feature.log",
  "task-3-wayland.ipc-default.json",
  "task-3-wayland.ipc-feature.json",
  "task-3-wayland.capture-attempts.tsv",
  "task-3-wayland.screenshooter.log",
  "task-3-wayland.capture.png",
  "task-3-wayland.capture-validation.txt",
  "task-3-wayland.cleanup.txt",
];

function createEvidenceFixture() {
  const runDirectory = mkdtempSync(join(tmpdir(), "wayland-integrity-"));
  for (const artifactName of artifactNames) writeFileSync(join(runDirectory, artifactName), `${artifactName}\n`);
  return runDirectory;
}

function runIntegrity(mode, runDirectory) {
  return execFileSync("node", [script, mode, runDirectory], { encoding: "utf8", stdio: "pipe" });
}

test("Wayland evidence integrity generator produces a review that verifies every listed artifact", () => {
  const runDirectory = createEvidenceFixture();
  try {
    assert.equal(runIntegrity("generate", runDirectory), "");
    assert.equal(runIntegrity("verify", runDirectory), "");
  } finally {
    rmSync(runDirectory, { recursive: true, force: true });
  }
});

test("Wayland evidence integrity verifier rejects an artifact after its generated review becomes stale", () => {
  const runDirectory = createEvidenceFixture();
  try {
    runIntegrity("generate", runDirectory);
    writeFileSync(join(runDirectory, "task-3-wayland.capture.png"), "tampered capture\n");
    assert.throws(
      () => runIntegrity("verify", runDirectory),
      /integrity mismatch for task-3-wayland\.capture\.png/,
    );
  } finally {
    rmSync(runDirectory, { recursive: true, force: true });
  }
});

test("Wayland evidence integrity verifier rejects a stale review hash", () => {
  const runDirectory = createEvidenceFixture();
  const reviewPath = join(runDirectory, "task-3-wayland.review.md");
  try {
    runIntegrity("generate", runDirectory);
    const review = readFileSync(reviewPath, "utf8");
    writeFileSync(reviewPath, review.replace(/[a-f0-9]{64}/, "0".repeat(64)));
    assert.throws(
      () => runIntegrity("verify", runDirectory),
      /review hash list does not match current artifacts/,
    );
  } finally {
    rmSync(runDirectory, { recursive: true, force: true });
  }
});

test("Wayland evidence integrity verifier rejects a finalization lock that remains in the run", () => {
  const runDirectory = createEvidenceFixture();
  try {
    runIntegrity("generate", runDirectory);
    writeFileSync(join(runDirectory, "task-3-wayland.integrity.lock"), "interrupted finalization\n");
    assert.throws(
      () => runIntegrity("verify", runDirectory),
      /manifest file list does not match current artifacts/,
    );
  } finally {
    rmSync(runDirectory, { recursive: true, force: true });
  }
});
