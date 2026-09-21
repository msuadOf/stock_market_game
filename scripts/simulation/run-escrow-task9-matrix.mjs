#!/usr/bin/env node

import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import {
  lstat,
  mkdir,
  readFile,
  realpath,
  writeFile,
} from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { verifyConservationSnapshot, verifyEvidenceBundle } from "./escrow-verification-contracts.mjs";
import { escrowSourceManifest } from "./escrow-source-manifest.mjs";

const SCHEMA = "escrow-task9-matrix-summary-v1";
const MATRIX_VERSION = "task9-runtime-v1-matrix-v1";
const CAPTURE_SCHEMA = "escrow-runtime-evidence-capture-v1";
const SCENARIO = "task9-runtime-v1";
const BUDGETS = ["1", "2", "4", "auto"];
const REPEATS = ["0", "1"];
const MODES = ["canonical", "perturbed"];
const NEGATIVE_DIMENSIONS = ["account", "stock", "completion"];
const REQUIRED_ARTIFACTS = ["authoritative_state", "event_stream", "receipts", "save_slot"];

class MatrixFailure extends Error {
  constructor(code, message, details = {}) {
    super(message);
    this.name = "MatrixFailure";
    this.code = code;
    this.details = details;
  }
}

export function sha256Hex(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function matrixEntries(seed) {
  const entries = [];
  for (const budget of BUDGETS) {
    for (const repeat of REPEATS) {
      for (const mode of MODES) {
        entries.push({
          id: `budget-${budget}-repeat-${repeat}-${mode}`,
          scenario: SCENARIO,
          seed,
          budget,
          repeat,
          mode,
          disabledMerge: null,
        });
      }
    }
  }
  for (const disabledMerge of NEGATIVE_DIMENSIONS) {
    entries.push({
      id: `negative-control-${disabledMerge}`,
      scenario: SCENARIO,
      seed,
      budget: "4",
      repeat: "0",
      mode: "negative-control",
      disabledMerge,
    });
  }
  return entries;
}

function isBelow(candidate, parent) {
  const relative = path.relative(parent, candidate);
  return relative !== "" && relative !== ".." && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative);
}

function isAtOrBelow(candidate, parent) {
  return candidate === parent || isBelow(candidate, parent);
}

function requireAbsoluteNormalized(label, value) {
  if (typeof value !== "string" || value.length === 0 || !path.isAbsolute(value) || path.resolve(value) !== value) {
    throw new MatrixFailure("INVALID_PATH", `${label} must be an absolute normalized path`, { label, value });
  }
}

async function existingDirectory(label, value) {
  requireAbsoluteNormalized(label, value);
  let resolved;
  try {
    resolved = await realpath(value);
  } catch (error) {
    throw new MatrixFailure("INVALID_PATH", `${label} is not an existing directory: ${error.message}`, { label, value });
  }
  if (resolved !== value) {
    throw new MatrixFailure("INVALID_PATH", `${label} must not traverse a symbolic link or alias`, { label, value, resolved });
  }
  const stat = await lstat(resolved);
  if (!stat.isDirectory()) {
    throw new MatrixFailure("INVALID_PATH", `${label} is not a directory`, { label, value });
  }
  return resolved;
}

async function normalizeConfig(config) {
  const workspaceRoot = await existingDirectory("workspaceRoot", config.workspaceRoot);
  const tempRoot = await existingDirectory("workspace .tmp", path.join(workspaceRoot, ".tmp"));
  const sourceRoot = await existingDirectory("sourceRoot", config.sourceRoot);
  const targetDir = await existingDirectory("targetDir", config.targetDir);
  const processTemp = await existingDirectory("processTemp", config.processTemp);
  const logsDir = await existingDirectory("logsDir", config.logsDir);
  if (!isAtOrBelow(sourceRoot, workspaceRoot)) {
    throw new MatrixFailure("INVALID_PATH", "sourceRoot must resolve at or below workspaceRoot");
  }
  for (const [label, value] of [["targetDir", targetDir], ["processTemp", processTemp], ["logsDir", logsDir]]) {
    if (!isBelow(value, tempRoot)) {
      throw new MatrixFailure("INVALID_PATH", `${label} must resolve below workspace .tmp`, { label, value, tempRoot });
    }
  }
  requireAbsoluteNormalized("outputRoot", config.outputRoot);
  const outputParent = await existingDirectory("outputRoot parent", path.dirname(config.outputRoot));
  if (!isBelow(outputParent, tempRoot) && outputParent !== tempRoot) {
    throw new MatrixFailure("INVALID_PATH", "outputRoot parent must resolve below workspace .tmp", { outputParent, tempRoot });
  }
  const outputRoot = path.join(outputParent, path.basename(config.outputRoot));
  if (!isBelow(outputRoot, tempRoot)) {
    throw new MatrixFailure("INVALID_PATH", "outputRoot must resolve below workspace .tmp", { outputRoot, tempRoot });
  }
  if (typeof config.seed !== "string" || !/^(0|[1-9][0-9]*)$/.test(config.seed)) {
    throw new MatrixFailure("INVALID_CONFIG", "seed must be an unsigned decimal string");
  }
  if (typeof config.sourceFingerprint !== "string" || !/^[0-9a-f]{64}$/.test(config.sourceFingerprint)) {
    throw new MatrixFailure("INVALID_CONFIG", "sourceFingerprint must be a lowercase SHA-256");
  }
  let evidence = null;
  if (config.evidence !== undefined) {
    if (config.evidence === null || typeof config.evidence !== "object" || Array.isArray(config.evidence)) {
      throw new MatrixFailure("INVALID_CONFIG", "evidence must be an object when supplied");
    }
    const paths = ["corpusDiffPath", "perfReportPath", "verificationBundlePath"];
    if (paths.some((key) => typeof config.evidence[key] !== "string")) {
      throw new MatrixFailure("INVALID_CONFIG", "evidence requires corpusDiffPath, perfReportPath, and verificationBundlePath");
    }
    const corpusDiffPath = await existingRegularFile("corpusDiffPath", config.evidence.corpusDiffPath, workspaceRoot);
    const perfReportPath = await existingRegularFile("perfReportPath", config.evidence.perfReportPath, workspaceRoot);
    const verificationBundlePath = await existingRegularFile("verificationBundlePath", config.evidence.verificationBundlePath, workspaceRoot);
    evidence = {
      corpusDiffPath,
      perfReportPath,
      verificationBundlePath,
      corpusDiffReceipt: await sealedInputReceipt(corpusDiffPath, "corpus diff"),
      perfReportReceipt: await sealedInputReceipt(perfReportPath, "perf report"),
      verificationBundleReceipt: await sealedInputReceipt(verificationBundlePath, "verification bundle"),
    };
  }
  return {
    workspaceRoot,
    sourceRoot,
    tempRoot,
    outputRoot,
    targetDir,
    processTemp,
    logsDir,
    seed: config.seed,
    sourceFingerprint: config.sourceFingerprint,
    evidence,
  };
}

function requestRecord(config) {
  return {
    matrix_version: MATRIX_VERSION,
    scenario: SCENARIO,
    seed: config.seed,
    source_fingerprint: config.sourceFingerprint,
    source_root: config.sourceRoot,
    workspace_root: config.workspaceRoot,
    output_root: config.outputRoot,
    cargo_target_dir: config.targetDir,
    process_temp: config.processTemp,
    logs_dir: config.logsDir,
    budgets: BUDGETS,
    repeats: REPEATS,
    modes: MODES,
    negative_controls: NEGATIVE_DIMENSIONS,
    complete_evidence_requested: config.evidence !== null,
  };
}

function requestFingerprint(request) {
  return sha256Hex(Buffer.from(JSON.stringify(request)));
}

async function pathExists(value) {
  try {
    await lstat(value);
    return true;
  } catch (error) {
    if (error.code === "ENOENT") return false;
    throw error;
  }
}

async function existingRegularFile(label, value, workspaceRoot) {
  requireAbsoluteNormalized(label, value);
  if (!isBelow(value, workspaceRoot)) {
    throw new MatrixFailure("INVALID_PATH", `${label} must resolve below workspaceRoot`, { label, value });
  }
  let canonical;
  try {
    canonical = await realpath(value);
    const stat = await lstat(canonical);
    if (!stat.isFile()) throw new Error("not a regular file");
  } catch (error) {
    throw new MatrixFailure("INVALID_PATH", `${label} is not an existing regular file: ${error.message}`, { label, value });
  }
  if (canonical !== value) {
    throw new MatrixFailure("INVALID_PATH", `${label} must not traverse a symbolic link or alias`, { label, value, canonical });
  }
  return canonical;
}

async function sealedInputReceipt(file, label) {
  const bytes = await readFile(file);
  if (bytes.length === 0) throw new MatrixFailure("MISSING_EVIDENCE", `${label} is empty`, { label, file });
  return { sha256: sha256Hex(bytes), byte_length: String(bytes.length) };
}

export async function assembleTask9Evidence({ workspaceRoot, outputRoot, evidence }) {
  const required = ["corpusDiffPath", "perfReportPath", "verificationBundlePath"];
  if (!evidence || required.some((key) => typeof evidence[key] !== "string")) {
    throw new MatrixFailure("MISSING_EVIDENCE", "complete Task 9 evidence requires corpus diff, perf report, and verification bundle paths");
  }
  const [corpusPath, perfPath, bundlePath] = await Promise.all([
    existingRegularFile("corpusDiffPath", evidence.corpusDiffPath, workspaceRoot),
    existingRegularFile("perfReportPath", evidence.perfReportPath, workspaceRoot),
    existingRegularFile("verificationBundlePath", evidence.verificationBundlePath, workspaceRoot),
  ]);
  const [corpus, perf, bundle] = await Promise.all([readFile(corpusPath), readFile(perfPath), readFile(bundlePath)]);
  for (const [label, bytes, receipt] of [
    ["corpus diff", corpus, evidence.corpusDiffReceipt],
    ["perf report", perf, evidence.perfReportReceipt],
    ["verification bundle", bundle, evidence.verificationBundleReceipt],
  ]) {
    if (receipt !== undefined && (sha256Hex(bytes) !== receipt.sha256 || String(bytes.length) !== receipt.byte_length)) {
      throw new MatrixFailure("ARTIFACT_HASH_DRIFT", `${label} changed after Task 9 matrix configuration was frozen`);
    }
  }
  let perfJson;
  let bundleJson;
  let corpusJson;
  try { corpusJson = JSON.parse(corpus); } catch (error) { throw new MatrixFailure("INVALID_EVIDENCE", `corpus diff is not JSON: ${error.message}`); }
  try { perfJson = JSON.parse(perf); } catch (error) { throw new MatrixFailure("INVALID_EVIDENCE", `perf report is not JSON: ${error.message}`); }
  try { bundleJson = JSON.parse(bundle); } catch (error) { throw new MatrixFailure("INVALID_EVIDENCE", `verification bundle is not JSON: ${error.message}`); }
  if (corpusJson.schema !== "task-9-corpus-diff-v1") {
    throw new MatrixFailure("INVALID_EVIDENCE", "corpus diff schema is unsupported");
  }
  if (perfJson.schema !== "escrow-perf-report-v3" || perfJson.status !== "PASS") {
    throw new MatrixFailure("INVALID_EVIDENCE", "perf report must be escrow-perf-report-v3 PASS");
  }
  try {
    verifyEvidenceBundle(bundleJson);
  } catch (error) {
    throw new MatrixFailure("INVALID_EVIDENCE", `verification bundle does not satisfy Task 9 contracts: ${error.message}`);
  }
  const files = [
    ["corpus-diff.json", corpus],
    ["perf-report.json", perf],
    ["verification-bundle.json", bundle],
  ];
  const artifacts = {};
  for (const [name, bytes] of files) {
    if (bytes.length === 0) throw new MatrixFailure("MISSING_EVIDENCE", `${name} is empty`);
    await writeFile(path.join(outputRoot, name), bytes, { flag: "wx" });
    artifacts[name] = { sha256: sha256Hex(bytes), byte_length: String(bytes.length) };
  }
  const complete = {
    schema: "escrow-task9-complete-evidence-v1",
    status: "PASS",
    artifacts,
    inputs: {
      corpus_diff: { path: corpusPath, receipt: evidence.corpusDiffReceipt ?? await sealedInputReceipt(corpusPath, "corpus diff") },
      perf_report: { path: perfPath, receipt: evidence.perfReportReceipt ?? await sealedInputReceipt(perfPath, "perf report") },
      verification_bundle: { path: bundlePath, receipt: evidence.verificationBundleReceipt ?? await sealedInputReceipt(bundlePath, "verification bundle") },
    },
  };
  const receiptBytes = Buffer.from(`${JSON.stringify(complete, null, 2)}\n`);
  await writeFile(path.join(outputRoot, "task9-evidence-receipts.json"), receiptBytes, { flag: "wx" });
  return { ...complete, receipt: { sha256: sha256Hex(receiptBytes), byte_length: String(receiptBytes.length) } };
}

async function validateAssembledTask9Evidence(outputRoot, expected) {
  if (expected === null) return null;
  if (expected === undefined) throw new MatrixFailure("MISSING_EVIDENCE", "completed matrix has no complete Task 9 evidence bundle");
  const receiptPath = path.join(outputRoot, "task9-evidence-receipts.json");
  const bytes = await readFile(receiptPath);
  if (sha256Hex(bytes) !== expected.receipt.sha256 || String(bytes.length) !== expected.receipt.byte_length) {
    throw new MatrixFailure("ARTIFACT_HASH_DRIFT", "complete Task 9 evidence receipt changed");
  }
  let receipt;
  try { receipt = JSON.parse(bytes); } catch (error) { throw new MatrixFailure("INVALID_EVIDENCE", `complete Task 9 evidence receipt is not JSON: ${error.message}`); }
  if (receipt.schema !== "escrow-task9-complete-evidence-v1" || receipt.status !== "PASS"
    || JSON.stringify(receipt.artifacts) !== JSON.stringify(expected.artifacts)) {
    throw new MatrixFailure("INVALID_EVIDENCE", "complete Task 9 evidence receipt content changed");
  }
  for (const [name, declared] of Object.entries(receipt.artifacts)) {
    const artifact = path.join(outputRoot, name);
    const artifactBytes = await readFile(artifact);
    if (sha256Hex(artifactBytes) !== declared.sha256 || String(artifactBytes.length) !== declared.byte_length) {
      throw new MatrixFailure("ARTIFACT_HASH_DRIFT", `complete Task 9 artifact ${name} changed`);
    }
  }
  return expected;
}

function normalizeStatus(value) {
  return typeof value === "string" ? value.toUpperCase() : null;
}

function parseStdoutSummary(stdout, entry) {
  let value;
  try {
    value = JSON.parse(stdout.trim());
  } catch (error) {
    throw new MatrixFailure("INVALID_STDOUT", `${entry.id} stdout is not one JSON object: ${error.message}`, { entry: entry.id });
  }
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new MatrixFailure("INVALID_STDOUT", `${entry.id} stdout summary must be an object`, { entry: entry.id });
  }
  return value;
}

function assertCaptureConfiguration(capture, entry) {
  const expected = {
    scenario: entry.scenario,
    seed: entry.seed,
    budget: entry.budget,
    repeat: entry.repeat,
    mode: entry.mode,
    requested_scheduler_merge_disabled: entry.disabledMerge,
  };
  const actual = capture.configuration;
  if (actual === null || typeof actual !== "object" || Array.isArray(actual)) {
    throw new MatrixFailure("CAPTURE_CONFIG_MISMATCH", `${entry.id} omitted capture.configuration`, { entry: entry.id });
  }
  for (const [key, value] of Object.entries(expected)) {
    if (actual[key] !== value) {
      throw new MatrixFailure("CAPTURE_CONFIG_MISMATCH", `${entry.id} configuration.${key} does not match the invocation`, {
        entry: entry.id,
        key,
        expected: value,
        actual: actual[key],
      });
    }
  }
}

function safeArtifactPath(output, file, entry, name) {
  if (typeof file !== "string" || file.length === 0 || path.isAbsolute(file) || path.normalize(file) !== file || file.split(path.sep).includes("..")) {
    throw new MatrixFailure("INVALID_ARTIFACT_PATH", `${entry.id} artifact ${name} has an unsafe file path`, { entry: entry.id, name, file });
  }
  const resolved = path.resolve(output, file);
  if (!isBelow(resolved, output)) {
    throw new MatrixFailure("INVALID_ARTIFACT_PATH", `${entry.id} artifact ${name} escapes its fresh output directory`, { entry: entry.id, name, file });
  }
  return resolved;
}

async function validateArtifacts(capture, output, entry) {
  const declarations = capture.artifacts;
  if (declarations === null || typeof declarations !== "object" || Array.isArray(declarations) || Object.keys(declarations).length === 0) {
    throw new MatrixFailure("MISSING_ARTIFACTS", `${entry.id} has no declared artifacts`, { entry: entry.id });
  }
  const declaredNames = Object.keys(declarations).sort();
  if (JSON.stringify(declaredNames) !== JSON.stringify([...REQUIRED_ARTIFACTS].sort())) {
    throw new MatrixFailure("MISSING_ARTIFACTS", `${entry.id} must declare exactly the Task 9 authoritative state, event stream, receipts, and save-slot artifacts`, {
      entry: entry.id,
      expected: REQUIRED_ARTIFACTS,
      actual: declaredNames,
    });
  }
  const seenFiles = new Set();
  const verified = [];
  for (const name of declaredNames) {
    const declaration = declarations[name];
    const receipt = declaration?.receipt;
    const file = declaration?.file;
    if (receipt === null || typeof receipt !== "object" || !/^[0-9a-f]{64}$/.test(receipt.sha256) || !/^(0|[1-9][0-9]*)$/.test(receipt.byte_length)) {
      throw new MatrixFailure("INVALID_ARTIFACT_RECEIPT", `${entry.id} artifact ${name} has an invalid receipt`, { entry: entry.id, name });
    }
    if (seenFiles.has(file)) {
      throw new MatrixFailure("INVALID_ARTIFACT_RECEIPT", `${entry.id} declares artifact file ${file} more than once`, { entry: entry.id, file });
    }
    seenFiles.add(file);
    const absolute = safeArtifactPath(output, file, entry, name);
    let canonical;
    let bytes;
    try {
      canonical = await realpath(absolute);
      bytes = await readFile(canonical);
    } catch (error) {
      throw new MatrixFailure("MISSING_ARTIFACT", `${entry.id} cannot read artifact ${name}: ${error.message}`, { entry: entry.id, name, file });
    }
    if (!isBelow(canonical, output)) {
      throw new MatrixFailure("INVALID_ARTIFACT_PATH", `${entry.id} artifact ${name} resolves outside its output directory`, { entry: entry.id, name, file, canonical });
    }
    const actualHash = sha256Hex(bytes);
    if (actualHash !== receipt.sha256 || String(bytes.length) !== receipt.byte_length) {
      throw new MatrixFailure("ARTIFACT_HASH_DRIFT", `${entry.id} artifact ${name} does not match its declared receipt`, {
        entry: entry.id,
        name,
        expected_sha256: receipt.sha256,
        actual_sha256: actualHash,
        expected_byte_length: receipt.byte_length,
        actual_byte_length: String(bytes.length),
      });
    }
    verified.push({ name, file, sha256: actualHash, byte_length: String(bytes.length) });
  }
  return verified;
}

function negativeControlDetected(capture) {
  const negative = capture.negative_control;
  return negative?.detected === true && negative.kind === "typed-rejection-with-rollback"
    && negative.enabled_merge_same_step_passed === true && negative.failed_step_published_events === "0"
    && typeof negative.error === "string" && negative.error.length > 0
    && negative.before_failed_step?.sha256 === negative.after_failed_step?.sha256
    && /^[0-9a-f]{64}$/.test(negative.before_failed_step?.sha256)
    && negative.before_failed_step?.byte_length === negative.after_failed_step?.byte_length;
}

function validateExecutorEvidence(capture, entry) {
  const scheduler = capture.scheduler_precanonical_order;
  if (scheduler?.available !== true || !Array.isArray(scheduler.records) || scheduler.records.length === 0) {
    throw new MatrixFailure("MISSING_EXECUTOR_EVIDENCE", `${entry.id} has no actual executor records`);
  }
  const groups = {
    account: ["P3AccountShards"],
    stock: ["P4AuctionStockShards", "P4ContinuousStockShards"],
    completion: ["P3WorkerResults", "P4AuctionWorkerResults", "P4ContinuousWorkerResults", "P5ReceiptResults"],
  };
  const orders = {};
  for (const [dimension, boundaries] of Object.entries(groups)) {
    if (entry.mode === "negative-control" && dimension !== entry.disabledMerge) continue;
    const record = scheduler.records.find((row) => boundaries.includes(row.boundary)
      && Array.isArray(row.identities) && row.identities.length >= 2
      && row.identities.every((identity) => typeof identity === "string" && identity.length > 0)
      && new Set(row.identities).size === row.identities.length
      && Array.isArray(row.item_counts) && row.item_counts.length === row.identities.length
      && row.item_counts.every((count) => Number.isSafeInteger(count) && count > 0));
    if (!record) throw new MatrixFailure("EMPTY_EXECUTOR_PERTURBATION", `${entry.id} lacks two nonempty actual ${dimension} identities`);
    orders[dimension] = record.identities;
  }
  if (entry.mode === "negative-control") {
    if (!negativeControlDetected(capture) || capture.negative_control.disabled_merge !== entry.disabledMerge) {
      throw new MatrixFailure("NEGATIVE_CONTROL_NOT_DETECTED", `${entry.id} has no bound rejection/rollback/control witness`);
    }
  } else {
    const snapshots = capture.producer_readiness?.conservation_snapshots;
    if (!Array.isArray(snapshots) || snapshots.length === 0) throw new MatrixFailure("MISSING_CONSERVATION", `${entry.id} has no per-tick conservation`);
    for (const snapshot of snapshots) verifyConservationSnapshot(snapshot);
    const from = BigInt(capture.runtime_coverage.tick_from);
    const to = BigInt(capture.runtime_coverage.tick_to);
    if (BigInt(snapshots.length) !== to - from + 1n || snapshots.some((snapshot, index) =>
      BigInt(snapshot.tick) !== from + BigInt(index) || snapshot.scenario !== entry.scenario || snapshot.seed !== entry.seed)) {
      throw new MatrixFailure("MISSING_CONSERVATION", `${entry.id} conservation omits, duplicates or misidentifies a committed tick`);
    }
  }
  return orders;
}

function verifyOrders(ordersBySlot, entry, orders) {
  const slot = `${entry.budget}/${entry.repeat}`;
  if (entry.mode === "canonical") { ordersBySlot.set(slot, orders); return; }
  if (entry.mode !== "perturbed") return;
  const canonical = ordersBySlot.get(slot);
  if (!canonical) throw new MatrixFailure("MISSING_EXECUTOR_EVIDENCE", `no canonical executor reference for ${entry.id}`);
  for (const dimension of ["account", "stock", "completion"]) {
    if (JSON.stringify(orders[dimension]) === JSON.stringify(canonical[dimension])
      || JSON.stringify([...orders[dimension]].sort()) !== JSON.stringify([...canonical[dimension]].sort())) {
      throw new MatrixFailure("INERT_EXECUTOR_PERTURBATION", `${entry.id} did not permute the same actual ${dimension} identities`);
    }
  }
}

function artifactVector(artifacts) {
  return JSON.stringify(artifacts.map(({ name, file, sha256, byte_length }) => ({ name, file, sha256, byte_length })));
}

async function defaultRunChild({ command, args, cwd, env }) {
  return await new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd, env, shell: false, stdio: ["ignore", "pipe", "pipe"] });
    const stdout = [];
    const stderr = [];
    child.stdout.on("data", (chunk) => stdout.push(chunk));
    child.stderr.on("data", (chunk) => stderr.push(chunk));
    child.on("error", reject);
    child.on("close", (code, signal) => resolve({
      code,
      signal,
      stdout: Buffer.concat(stdout).toString("utf8"),
      stderr: Buffer.concat(stderr).toString("utf8"),
    }));
  });
}

function harnessInvocation(entry, output) {
  const harnessArgs = [
    "--scenario", entry.scenario,
    "--seed", entry.seed,
    "--budget", entry.budget,
    "--repeat", entry.repeat,
    "--mode", entry.mode,
  ];
  if (entry.disabledMerge !== null) harnessArgs.push("--disable-merge", entry.disabledMerge);
  harnessArgs.push("--output", output);
  return {
    command: "cargo",
    args: ["run", "--quiet", "-p", "engine", "--features", "verification-harness", "--example", "escrow_verification_harness", "--", ...harnessArgs],
    printable: ["cargo", "run", "--quiet", "-p", "engine", "--features", "verification-harness", "--example", "escrow_verification_harness", "--", ...harnessArgs],
  };
}

async function runEntry(config, entry, runChild, logPrefix) {
  const output = path.join(config.outputRoot, "runs", entry.id);
  if (await pathExists(output)) {
    throw new MatrixFailure("OUTPUT_NOT_FRESH", `${entry.id} output already exists`, { entry: entry.id, output });
  }
  const invocation = harnessInvocation(entry, output);
  const env = {
    ...process.env,
    ESCROW_WORKSPACE_ROOT: config.workspaceRoot,
    CARGO_TARGET_DIR: config.targetDir,
    TMPDIR: config.processTemp,
    TMP: config.processTemp,
    TEMP: config.processTemp,
  };
  const result = await runChild({ ...invocation, cwd: config.sourceRoot, env, entry, output });
  const stdoutLog = path.join(config.logsDir, `${logPrefix}-${entry.id}.stdout.log`);
  const stderrLog = path.join(config.logsDir, `${logPrefix}-${entry.id}.stderr.log`);
  await Promise.all([
    writeFile(stdoutLog, result.stdout ?? "", { flag: "wx" }),
    writeFile(stderrLog, result.stderr ?? "", { flag: "wx" }),
  ]);
  if (!(await pathExists(path.join(output, "capture.json")))) {
    throw new MatrixFailure("MISSING_CAPTURE", `${entry.id} did not create capture.json`, { entry: entry.id, exit_code: result.code, signal: result.signal });
  }
  let capture;
  try {
    capture = JSON.parse(await readFile(path.join(output, "capture.json"), "utf8"));
  } catch (error) {
    throw new MatrixFailure("INVALID_CAPTURE", `${entry.id} capture.json is invalid: ${error.message}`, { entry: entry.id });
  }
  const stdout = parseStdoutSummary(result.stdout ?? "", entry);
  if (capture.schema !== CAPTURE_SCHEMA) {
    throw new MatrixFailure("INVALID_CAPTURE", `${entry.id} capture schema is unsupported`, { entry: entry.id, schema: capture.schema });
  }
  const expectedCapturePath = path.join(output, "capture.json");
  if (stdout.capture !== expectedCapturePath) {
    throw new MatrixFailure("STATUS_MISMATCH", `${entry.id} stdout does not identify its exact capture.json`, {
      entry: entry.id,
      expected_capture: expectedCapturePath,
      actual_capture: stdout.capture,
    });
  }
  const captureStatus = normalizeStatus(capture.status);
  const stdoutStatus = normalizeStatus(stdout.status);
  if (captureStatus === "BLOCKED" || stdoutStatus === "BLOCKED" || result.code === 3) {
    throw new MatrixFailure("HARNESS_BLOCKED", `${entry.id} is BLOCKED and cannot count as Task 9 PASS`, { entry: entry.id, exit_code: result.code, capture_status: capture.status, stdout_status: stdout.status });
  }
  if (captureStatus === "FAIL" || stdoutStatus === "FAIL") {
    throw new MatrixFailure("HARNESS_FAIL", `${entry.id} harness reported FAIL`, { entry: entry.id, exit_code: result.code, capture_status: capture.status, stdout_status: stdout.status });
  }
  if (captureStatus !== stdoutStatus) {
    throw new MatrixFailure("STATUS_MISMATCH", `${entry.id} stdout and capture status disagree`, { entry: entry.id, capture_status: capture.status, stdout_status: stdout.status });
  }
  if (captureStatus !== "PASS") {
    throw new MatrixFailure("INVALID_STATUS", `${entry.id} status is neither PASS, FAIL, nor BLOCKED`, { entry: entry.id, capture_status: capture.status });
  }
  if (result.code !== 0 || result.signal !== null) {
    throw new MatrixFailure("EXIT_STATUS_MISMATCH", `${entry.id} reported PASS but did not exit successfully`, { entry: entry.id, exit_code: result.code, signal: result.signal });
  }
  assertCaptureConfiguration(capture, entry);
  const artifacts = await validateArtifacts(capture, output, entry);
  const executorOrders = validateExecutorEvidence(capture, entry);
  if (entry.mode === "negative-control" && !negativeControlDetected(capture)) {
    throw new MatrixFailure("NEGATIVE_CONTROL_NOT_DETECTED", `${entry.id} did not prove that disabling its canonical merge was detected`, { entry: entry.id, dimension: entry.disabledMerge });
  }
  return {
    id: entry.id,
    status: "PASS",
    exit_code: result.code,
    output: path.relative(config.outputRoot, output).split(path.sep).join("/"),
    stdout_log: stdoutLog,
    stderr_log: stderrLog,
    command: invocation.printable,
    configuration: {
      scenario: entry.scenario,
      seed: entry.seed,
      budget: entry.budget,
      repeat: entry.repeat,
      mode: entry.mode,
      disabled_merge: entry.disabledMerge,
    },
    artifacts,
    executor_orders: executorOrders,
  };
}

async function validateStoredPass(config, request, requestHash, sourceManifest) {
  let summary;
  try {
    summary = JSON.parse(await readFile(path.join(config.outputRoot, "summary.json"), "utf8"));
  } catch (error) {
    throw new MatrixFailure("OUTPUT_NOT_FRESH", `existing output is not a completed reusable matrix: ${error.message}`);
  }
  if (summary.schema !== SCHEMA || summary.status !== "PASS" || summary.request_fingerprint !== requestHash || JSON.stringify(summary.request) !== JSON.stringify(request)) {
    throw new MatrixFailure("OUTPUT_NOT_FRESH", "existing output does not match this completed matrix request");
  }
  let storedSourceManifest;
  try {
    storedSourceManifest = JSON.parse(await readFile(path.join(config.outputRoot, "source-manifest.json"), "utf8"));
  } catch (error) {
    throw new MatrixFailure("REUSE_VALIDATION_FAILED", `stored source manifest is unreadable: ${error.message}`);
  }
  if (JSON.stringify(storedSourceManifest) !== JSON.stringify(sourceManifest)) {
    throw new MatrixFailure("SOURCE_HASH_DRIFT", "stored source manifest differs from the current frozen source bytes");
  }
  const expected = matrixEntries(config.seed);
  if (!Array.isArray(summary.entries) || summary.entries.length !== expected.length) {
    throw new MatrixFailure("REUSE_VALIDATION_FAILED", "stored PASS summary has an incomplete matrix");
  }
  const lines = [];
  let baselineVector = null;
  const ordersBySlot = new Map();
  for (const [index, stored] of summary.entries.entries()) {
    if (stored.id !== expected[index].id || stored.status !== "PASS") {
      throw new MatrixFailure("REUSE_VALIDATION_FAILED", "stored PASS matrix order or entry status changed", { index });
    }
    const output = path.resolve(config.outputRoot, stored.output);
    if (!isBelow(output, config.outputRoot)) {
      throw new MatrixFailure("REUSE_VALIDATION_FAILED", "stored PASS entry output escapes the matrix root", { entry: stored.id });
    }
    const capture = JSON.parse(await readFile(path.join(output, "capture.json"), "utf8"));
    if (capture.schema !== CAPTURE_SCHEMA || normalizeStatus(capture.status) !== "PASS") {
      throw new MatrixFailure("REUSE_VALIDATION_FAILED", "stored capture no longer reports PASS", { entry: stored.id, status: capture.status });
    }
    assertCaptureConfiguration(capture, expected[index]);
    verifyOrders(ordersBySlot, expected[index], validateExecutorEvidence(capture, expected[index]));
    const artifacts = await validateArtifacts(capture, output, expected[index]);
    if (artifactVector(artifacts) !== artifactVector(stored.artifacts)) {
      throw new MatrixFailure("REUSE_VALIDATION_FAILED", "stored PASS artifact receipt list changed", { entry: stored.id });
    }
    const vector = artifactVector(artifacts);
    if (expected[index].mode !== "negative-control") {
      if (baselineVector === null) baselineVector = vector;
      else if (vector !== baselineVector) {
        throw new MatrixFailure("REUSE_VALIDATION_FAILED", "stored deterministic artifact receipts drifted", { entry: stored.id });
      }
    } else {
      if (!negativeControlDetected(capture) || vector === baselineVector) {
        throw new MatrixFailure("REUSE_VALIDATION_FAILED", "stored negative control no longer exposes the disabled merge", { entry: stored.id });
      }
    }
    for (const artifact of artifacts) lines.push(`${artifact.sha256}  ${stored.output}/${artifact.file}`);
  }
  const determinism = `${lines.join("\n")}\n`;
  const persisted = await readFile(path.join(config.outputRoot, "determinism.sha256"));
  if (persisted.toString("utf8") !== determinism || sha256Hex(persisted) !== summary.determinism_manifest_sha256) {
    throw new MatrixFailure("REUSE_VALIDATION_FAILED", "stored determinism.sha256 changed");
  }
  await validateAssembledTask9Evidence(config.outputRoot, summary.complete_evidence);
  return { ...summary, reused: true };
}

function failureRecord(error) {
  if (error instanceof MatrixFailure) {
    return { code: error.code, message: error.message, details: error.details };
  }
  return { code: "RUNNER_INTERNAL", message: error instanceof Error ? error.message : String(error), details: {} };
}

export async function runTask9Matrix(inputConfig, { runChild = defaultRunChild } = {}) {
  const config = await normalizeConfig(inputConfig);
  const sourceManifest = await escrowSourceManifest(config.sourceRoot);
  if (sourceManifest.sha256 !== config.sourceFingerprint) {
    throw new MatrixFailure("SOURCE_HASH_DRIFT", "source bytes do not match the requested complete source fingerprint");
  }
  const request = requestRecord(config);
  const requestHash = requestFingerprint(request);
  if (await pathExists(config.outputRoot)) {
    try {
      const canonical = await realpath(config.outputRoot);
      if (canonical !== config.outputRoot) throw new MatrixFailure("OUTPUT_NOT_FRESH", "existing outputRoot is a symbolic link or alias");
      return await validateStoredPass(config, request, requestHash, sourceManifest);
    } catch (error) {
      return {
        schema: SCHEMA,
        status: "FAIL",
        request,
        request_fingerprint: requestHash,
        reused: false,
        entries: [],
        determinism_manifest_sha256: null,
        failure: failureRecord(error),
      };
    }
  }
  await mkdir(path.join(config.outputRoot, "runs"), { recursive: true });
  await writeFile(path.join(config.outputRoot, "source-manifest.json"), `${JSON.stringify(sourceManifest, null, 2)}\n`, { flag: "wx" });
  const entries = [];
  const logPrefix = `${path.basename(config.outputRoot).replaceAll(/[^A-Za-z0-9_.-]/g, "-")}-${requestHash.slice(0, 12)}`;
  let baselineVector = null;
  const ordersBySlot = new Map();
  try {
    for (const entry of matrixEntries(config.seed)) {
      const result = await runEntry(config, entry, runChild, logPrefix);
      if ((await escrowSourceManifest(config.sourceRoot)).sha256 !== sourceManifest.sha256) {
        throw new MatrixFailure("SOURCE_HASH_DRIFT", `${entry.id} source changed while validation was running`);
      }
      verifyOrders(ordersBySlot, entry, result.executor_orders);
      if (entry.mode !== "negative-control") {
        const vector = artifactVector(result.artifacts);
        if (baselineVector === null) baselineVector = vector;
        else if (vector !== baselineVector) {
          throw new MatrixFailure("DETERMINISM_DRIFT", `${entry.id} artifact receipts differ from the frozen matrix reference`, { entry: entry.id });
        }
      } else if (artifactVector(result.artifacts) === baselineVector) {
        throw new MatrixFailure("NEGATIVE_CONTROL_NOT_DETECTED", `${entry.id} artifact receipts did not expose the disabled canonical merge`, {
          entry: entry.id,
          dimension: entry.disabledMerge,
        });
      }
      entries.push(result);
    }
    const lines = entries.flatMap((entry) => entry.artifacts.map((artifact) => `${artifact.sha256}  ${entry.output}/${artifact.file}`));
    const determinism = `${lines.join("\n")}\n`;
    await writeFile(path.join(config.outputRoot, "determinism.sha256"), determinism, { flag: "wx" });
    const summary = {
      schema: SCHEMA,
      status: "PASS",
      request,
      request_fingerprint: requestHash,
      reused: false,
      entries,
      determinism_manifest_sha256: sha256Hex(Buffer.from(determinism)),
      complete_evidence: null,
      failure: null,
    };
    if (config.evidence !== null) {
      summary.complete_evidence = await assembleTask9Evidence({
        workspaceRoot: config.workspaceRoot,
        outputRoot: config.outputRoot,
        evidence: config.evidence,
      });
    }
    await writeFile(path.join(config.outputRoot, "summary.json"), `${JSON.stringify(summary, null, 2)}\n`, { flag: "wx" });
    return summary;
  } catch (error) {
    const summary = {
      schema: SCHEMA,
      status: "FAIL",
      request,
      request_fingerprint: requestHash,
      reused: false,
      entries,
      determinism_manifest_sha256: null,
      failure: failureRecord(error),
    };
    await writeFile(path.join(config.outputRoot, "summary.json"), `${JSON.stringify(summary, null, 2)}\n`, { flag: "wx" });
    return summary;
  }
}

async function runGit(sourceRoot, args) {
  const result = await defaultRunChild({ command: "git", args, cwd: sourceRoot, env: process.env });
  if (result.code !== 0 || result.signal !== null) {
    throw new MatrixFailure("SOURCE_IDENTITY_FAILED", `git ${args.join(" ")} failed: ${result.stderr.trim()}`);
  }
  return result.stdout.trim();
}

async function frozenSourceFingerprint(sourceRoot) {
  return (await escrowSourceManifest(sourceRoot)).sha256;
}

function parseCli(argv) {
  const values = new Map();
  for (let index = 0; index < argv.length; index += 2) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (!flag?.startsWith("--") || value === undefined || values.has(flag)) {
      throw new MatrixFailure("INVALID_ARGUMENT", "usage: node scripts/simulation/run-escrow-task9-matrix.mjs --workspace-root <absolute> --source-root <absolute> --output <new-absolute-directory> --logs <absolute-existing-directory> --seed <u64> [--corpus-diff <file> --perf-report <file> --verification-bundle <file>]");
    }
    values.set(flag, value);
  }
  const allowed = new Set(["--workspace-root", "--source-root", "--output", "--logs", "--seed", "--corpus-diff", "--perf-report", "--verification-bundle"]);
  for (const flag of values.keys()) {
    if (!allowed.has(flag)) throw new MatrixFailure("INVALID_ARGUMENT", `unsupported argument ${flag}`);
  }
  for (const flag of ["--workspace-root", "--source-root", "--output", "--logs", "--seed"]) {
    if (!values.has(flag)) throw new MatrixFailure("INVALID_ARGUMENT", `${flag} is required`);
  }
  const evidenceFlags = ["--corpus-diff", "--perf-report", "--verification-bundle"];
  const suppliedEvidence = evidenceFlags.filter((flag) => values.has(flag)).length;
  if (suppliedEvidence !== 0 && suppliedEvidence !== evidenceFlags.length) {
    throw new MatrixFailure("INVALID_ARGUMENT", "--corpus-diff, --perf-report, and --verification-bundle must be supplied together");
  }
  return values;
}

async function main() {
  let summary;
  try {
    const args = parseCli(process.argv.slice(2));
    const sourceRoot = args.get("--source-root");
    const targetDir = process.env.CARGO_TARGET_DIR;
    const processTemp = process.env.TMPDIR;
    if (!targetDir || !processTemp || process.env.TMP !== processTemp || process.env.TEMP !== processTemp) {
      throw new MatrixFailure("INVALID_ENVIRONMENT", "CARGO_TARGET_DIR must be set and TMPDIR/TMP/TEMP must name the same workspace-local directory");
    }
    summary = await runTask9Matrix({
      workspaceRoot: args.get("--workspace-root"),
      sourceRoot,
      outputRoot: args.get("--output"),
      logsDir: args.get("--logs"),
      targetDir,
      processTemp,
      seed: args.get("--seed"),
      sourceFingerprint: await frozenSourceFingerprint(sourceRoot),
      evidence: args.has("--corpus-diff") ? {
        corpusDiffPath: args.get("--corpus-diff"),
        perfReportPath: args.get("--perf-report"),
        verificationBundlePath: args.get("--verification-bundle"),
      } : undefined,
    });
  } catch (error) {
    summary = { schema: SCHEMA, status: "FAIL", failure: failureRecord(error) };
  }
  process.stdout.write(`${JSON.stringify(summary, null, 2)}\n`);
  process.exitCode = summary.status === "PASS" ? 0 : 1;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  await main();
}
