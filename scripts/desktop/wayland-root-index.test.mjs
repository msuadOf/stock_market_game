import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import test from "node:test";

const evidenceRoot = resolve(".omo/evidence/resolve-blockers-wayland");
const indexPath = join(evidenceRoot, "task-3-wayland-evidence.md");
const rootReviewPaths = [
  join(evidenceRoot, "task-3-wayland-evidence.md"),
  join(evidenceRoot, "task-3-wayland-review.md"),
  join(evidenceRoot, "task-3-wayland-review-legacy.md"),
];
const recordCountWords = new Map([
  ["zero", 0],
  ["one", 1],
  ["two", 2],
  ["three", 3],
  ["four", 4],
  ["five", 5],
  ["six", 6],
]);

function selectedRunDirectory(index) {
  const match = /\[`(task-3-wayland\.run-[A-Za-z0-9-]+)`\]\(\1\/\)/.exec(index);
  if (!match) throw new Error("root index does not identify a selected immutable run");
  return match[1];
}

function immutableFeatureIpc(runDirectory) {
  const parsed = JSON.parse(readFileSync(join(evidenceRoot, runDirectory, "task-3-wayland.ipc-feature.json"), "utf8"));
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    throw new Error("immutable feature IPC is not an object");
  }
  if (!Object.hasOwn(parsed, "records") || !Number.isInteger(parsed.records)) {
    throw new Error("immutable feature IPC does not contain an integer records count");
  }
  return parsed.records;
}

function assertFeatureCountMatchesIndex(index, records) {
  const match = /hasRecordsProperty:true`,\s*(zero|one|two|three|four|five|six) records/.exec(index);
  if (!match) throw new Error("root index does not state a parseable feature record count");
  const rootCount = recordCountWords.get(match[1]);
  if (rootCount !== records) {
    throw new Error(`root index claims ${match[1]} records but immutable feature IPC records ${records}`);
  }
}

test("Task-3 root index matches the selected immutable feature IPC records count", () => {
  const index = readFileSync(indexPath, "utf8");
  assertFeatureCountMatchesIndex(index, immutableFeatureIpc(selectedRunDirectory(index)));
});

test("Task-3 root-index coherence check rejects stale prose record counts", () => {
  assert.throws(
    () => assertFeatureCountMatchesIndex("`hasRecordsProperty:true`, six records", 5),
    /root index claims six records but immutable feature IPC records 5/,
  );
});

test("Task-3 root review maps do not retain a stale six-record claim", () => {
  const stalePaths = rootReviewPaths.filter((path) => /\b(?:six|6) (?:bounded )?records\b/.test(readFileSync(path, "utf8")));
  assert.deepEqual(stalePaths, []);
});
