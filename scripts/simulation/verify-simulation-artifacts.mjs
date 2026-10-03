#!/usr/bin/env node
import { createHash } from "node:crypto";
import { writeSync } from "node:fs";
import fsp from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const MATRIX_SEEDS = [1, 2, 3, 4, 5, 7, 11, 19, 23, 31];
const CROSS_YEAR_SEEDS = [1, 7, 11, 19, 31];
const SENSITIVITY_MULTIPLIERS = [0.5, 1, 2];
const SIMULATION_PRIMARY_NATURAL_DAYS = 5;
const SIMULATION_CROSS_YEAR_NATURAL_DAYS = 8;
const SIMULATION_ORDINARY_TEST_MAX_MS = 10_000;
const SIMULATION_CHILD_TIMEOUT_MS = 300_000;
const SIMULATION_BATCH_TIMEOUT_MS = 300_000;
const SIMULATION_CLEANUP_RESERVE_MS = 1_000;
const SIMULATION_CHECKPOINT_SCHEMA = "simulation-acceptance-checkpoint";
const SIMULATION_RUNNER_ID = "simulation-acceptance";
const SIMULATION_SOURCE_FINGERPRINT_ALGORITHM = "simulation-source-fingerprint";
const SIMULATION_DETERMINISM_RECEIPT_SCHEMA = "simulation-determinism-receipt";
const SIMULATION_RESOURCE_POLICY_SCHEMA = "simulation-resource-policy";
const SIMULATION_MAX_CONCURRENT_CHILD_EXECUTIONS = 30;
const SIMULATION_MIN_RAYON_THREADS_PER_SEED = 4;
const REQUIRED_SOURCE_FILES = [
  "Cargo.lock",
  "Cargo.toml",
  "rust-toolchain.toml",
  "scripts/simulation/baseline-run.mjs",
  "scripts/simulation/verify-simulation-artifacts.mjs",
];

function sha256(content) {
  return createHash("sha256").update(content).digest("hex");
}

function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function fail(message) {
  throw new Error(message);
}

function requireRecord(value, label) {
  if (!isRecord(value)) fail(`${label} must be a JSON object`);
  return value;
}

function requireExactKeys(value, expected, label) {
  const actual = Object.keys(requireRecord(value, label)).sort();
  const wanted = [...expected].sort();
  if (JSON.stringify(actual) !== JSON.stringify(wanted)) {
    fail(`${label} keys mismatch: expected ${wanted.join(", ")}; got ${actual.join(", ")}`);
  }
}

function requireJsonEqual(actual, expected, label) {
  if (canonicalJson(actual) !== canonicalJson(expected)) fail(`${label} mismatch`);
}

function canonicalJson(value) {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  if (isRecord(value)) {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function requireSha256(value, label) {
  if (typeof value !== "string" || !/^[a-f0-9]{64}$/.test(value)) {
    fail(`${label} must be a lowercase SHA-256 digest`);
  }
}

function requireNonNegativeDuration(value, label) {
  if (!Number.isFinite(value) || value < 0) fail(`${label} must be a non-negative finite duration`);
}

function resolveContained(root, relativePath, label) {
  if (typeof relativePath !== "string" || relativePath.length === 0 || path.isAbsolute(relativePath) || relativePath.includes("\\")) {
    fail(`${label} must be a non-empty relative POSIX path`);
  }
  const components = relativePath.split("/");
  if (components.some((component) => component.length === 0 || component === "." || component === "..")) {
    fail(`${label} contains an unsafe path component`);
  }
  const resolved = path.resolve(root, ...components);
  if (resolved === root || !resolved.startsWith(`${root}${path.sep}`)) fail(`${label} escapes the simulation acceptance root`);
  return resolved;
}

async function requireRegularFile(filePath, label) {
  let stat;
  try {
    stat = await fsp.lstat(filePath);
  } catch (error) {
    if (error?.code === "ENOENT") fail(`${label} is missing: ${filePath}`);
    throw error;
  }
  if (!stat.isFile() || stat.isSymbolicLink()) fail(`${label} must be a regular file: ${filePath}`);
}

async function readJson(filePath, label) {
  await requireRegularFile(filePath, label);
  let bytes;
  try {
    bytes = await fsp.readFile(filePath);
  } catch (error) {
    fail(`${label} cannot be read: ${error.message}`);
  }
  let parsed;
  try {
    parsed = JSON.parse(bytes.toString("utf8"));
  } catch (error) {
    fail(`${label} is malformed JSON: ${error.message}`);
  }
  return { bytes, parsed };
}

async function requireDirectory(directoryPath, label) {
  let stat;
  try {
    stat = await fsp.lstat(directoryPath);
  } catch (error) {
    if (error?.code === "ENOENT") fail(`${label} is missing: ${directoryPath}`);
    throw error;
  }
  if (!stat.isDirectory() || stat.isSymbolicLink()) fail(`${label} must be a real directory: ${directoryPath}`);
}

async function requireExactDirectoryEntries(directoryPath, expectedNames, label) {
  const entries = await fsp.readdir(directoryPath, { withFileTypes: true });
  for (const entry of entries) {
    if (entry.isSymbolicLink()) fail(`${label} contains unsupported symlink: ${entry.name}`);
  }
  const actual = entries.map((entry) => entry.name).sort();
  const expected = [...expectedNames].sort();
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    const missing = expected.filter((name) => !actual.includes(name));
    const extra = actual.filter((name) => !expected.includes(name));
    fail(`${label} artifact set mismatch; missing=[${missing.join(", ")}], extra=[${extra.join(", ")}]`);
  }
}

function validateGit(git) {
  requireExactKeys(git, ["revision", "branch", "dirty_paths"], "manifest.git");
  if (typeof git.revision !== "string" || !/^[a-f0-9]{40,64}$/.test(git.revision)) fail("manifest.git.revision must be a Git object id");
  if (typeof git.branch !== "string") fail("manifest.git.branch must be a string");
  if (!Array.isArray(git.dirty_paths) || !git.dirty_paths.every((entry) => typeof entry === "string" && entry.length > 0)) {
    fail("manifest.git.dirty_paths must be an array of non-empty porcelain records");
  }
  if (new Set(git.dirty_paths).size !== git.dirty_paths.length) fail("manifest.git.dirty_paths contains duplicates");
}

function validateResourcePolicy(policy) {
  requireExactKeys(policy, ["schema", "schema_version", "available_cpu_count", "available_cpu_source", "maximum_thread_count", "max_concurrent_child_executions", "rayon_threads_per_seed", "ordinary_test_max_ms", "child_timeout_ms", "batch_timeout_ms", "execution_timeout_ms", "cleanup_reserve_ms"], "manifest.resource_policy");
  if (policy.schema !== SIMULATION_RESOURCE_POLICY_SCHEMA || policy.schema_version !== 7) fail(`unsupported simulation acceptance resource policy schema: ${JSON.stringify(policy.schema)}`);
  if (!Number.isInteger(policy.available_cpu_count) || policy.available_cpu_count <= 0) fail("resource policy CPU count must be positive");
  if (typeof policy.available_cpu_source !== "string" || policy.available_cpu_source.length === 0) fail("resource policy CPU source is missing");
  if (policy.maximum_thread_count !== "auto" && (!Number.isInteger(policy.maximum_thread_count) || policy.maximum_thread_count <= 0)) {
    fail("resource policy maximum thread count must be positive or auto");
  }
  const totalThreadBudget = policy.maximum_thread_count === "auto"
    ? policy.available_cpu_count
    : Math.min(policy.available_cpu_count, policy.maximum_thread_count);
  const expectedConcurrentSeeds = Math.min(
    SIMULATION_MAX_CONCURRENT_CHILD_EXECUTIONS,
    Math.max(1, Math.floor(totalThreadBudget / SIMULATION_MIN_RAYON_THREADS_PER_SEED)),
  );
  if (policy.max_concurrent_child_executions !== expectedConcurrentSeeds) fail("resource policy concurrent child budget is inconsistent");
  const expectedRayonThreads = Math.max(1, Math.floor(totalThreadBudget / expectedConcurrentSeeds));
  if (policy.rayon_threads_per_seed !== expectedRayonThreads) fail("resource policy Rayon thread budget is inconsistent");
  if (policy.max_concurrent_child_executions * policy.rayon_threads_per_seed > totalThreadBudget) {
    fail("resource policy aggregate Rayon budget exceeds its detected CPU limit");
  }
  if (policy.ordinary_test_max_ms !== SIMULATION_ORDINARY_TEST_MAX_MS
    || policy.child_timeout_ms !== SIMULATION_CHILD_TIMEOUT_MS
    || policy.batch_timeout_ms !== SIMULATION_BATCH_TIMEOUT_MS
    || policy.execution_timeout_ms !== SIMULATION_BATCH_TIMEOUT_MS - SIMULATION_CLEANUP_RESERVE_MS
    || policy.cleanup_reserve_ms !== SIMULATION_CLEANUP_RESERVE_MS) {
    fail("resource policy time limits do not match the sealed ten-second/five-minute contract");
  }
}

function sourceFingerprintState(fingerprint) {
  return {
    algorithm: fingerprint.algorithm, algorithm_version: fingerprint.algorithm_version,
    committed_tree: fingerprint.committed_tree,
    dirty_patch_sha256: fingerprint.dirty_patch_sha256,
    files: fingerprint.files,
  };
}

function validateSourceFingerprint(fingerprint) {
  requireExactKeys(fingerprint, ["algorithm", "algorithm_version", "committed_tree", "dirty_patch_sha256", "files", "digest"], "manifest.source_fingerprint");
  if (fingerprint.algorithm !== SIMULATION_SOURCE_FINGERPRINT_ALGORITHM || fingerprint.algorithm_version !== 1) fail(`unsupported source fingerprint algorithm: ${JSON.stringify(fingerprint.algorithm)}`);
  if (typeof fingerprint.committed_tree !== "string" || !/^[a-f0-9]{40,64}$/.test(fingerprint.committed_tree)) fail("source fingerprint committed tree is invalid");
  requireSha256(fingerprint.dirty_patch_sha256, "source fingerprint dirty patch digest");
  requireSha256(fingerprint.digest, "source fingerprint digest");
  if (!Array.isArray(fingerprint.files) || fingerprint.files.length === 0) fail("source fingerprint file list must be non-empty");
  const paths = [];
  for (const [index, entry] of fingerprint.files.entries()) {
    requireExactKeys(entry, ["path", "sha256"], `source fingerprint file ${index}`);
    if (typeof entry.path !== "string" || entry.path.length === 0 || path.posix.isAbsolute(entry.path)
      || entry.path.includes("\\") || entry.path.split("/").some((part) => part === "" || part === "." || part === "..")) {
      fail(`source fingerprint file ${index} has an unsafe path`);
    }
    requireSha256(entry.sha256, `source fingerprint file ${entry.path}`);
    paths.push(entry.path);
  }
  if (new Set(paths).size !== paths.length) fail("source fingerprint file list contains duplicate paths");
  for (const required of REQUIRED_SOURCE_FILES) {
    if (!paths.includes(required)) fail(`source fingerprint omits required input ${required}`);
  }
  if (!paths.some((entry) => entry.startsWith(".cargo/"))) fail("source fingerprint omits .cargo inputs");
  if (!paths.some((entry) => entry.startsWith("packages/engine/"))) fail("source fingerprint omits engine inputs");
  const calculated = sha256(JSON.stringify(sourceFingerprintState(fingerprint)));
  if (fingerprint.digest !== calculated) fail("source fingerprint digest mismatch");
}

function matrixDirectoryName(spec) {
  return `${spec.scenario}-behavior-${spec.behavior}-event-${spec.event}-volume-denominator-${spec.volumeDenominatorAssumption}`;
}

function validateFixtureBinary(fixture, resourcePolicy) {
  requireExactKeys(fixture, ["source", "executable_relative_path", "workspace_root", "cargo_target_dir", "process_tmp_dir", "binary_sha256", "binary_bytes", "embedded_source_fingerprint_digest", "build_argv", "cargo_build_jobs", "argv"], "manifest.fixture_binary");
  if (fixture.source !== "current_session_setup") fail("fixture binary source is invalid");
  if (typeof fixture.executable_relative_path !== "string"
    || path.posix.isAbsolute(fixture.executable_relative_path)
    || fixture.executable_relative_path.split("/").some((part) => part === "" || part === "." || part === "..")
    || !/simulation_baseline_fixture(?:\.exe)?$/.test(fixture.executable_relative_path)) fail("fixture binary executable path is invalid");
  requireSha256(fixture.binary_sha256, "fixture binary digest");
  requireSha256(fixture.embedded_source_fingerprint_digest, "fixture embedded source fingerprint");
  if (!Number.isSafeInteger(fixture.binary_bytes) || fixture.binary_bytes <= 0) fail("fixture binary byte length must be positive");
  requireJsonEqual(fixture.build_argv, ["cargo", "build", "-p", "engine", "--release", "--features", "simulation-diagnostics", "--example", "simulation_baseline_fixture", "--message-format=json-render-diagnostics"], "fixture binary build argv");
  if (!Number.isInteger(fixture.cargo_build_jobs) || fixture.cargo_build_jobs <= 0 || fixture.cargo_build_jobs > resourcePolicy.available_cpu_count) {
    fail("fixture binary Cargo build budget is invalid");
  }
  if (!path.isAbsolute(fixture.workspace_root)
    || fixture.cargo_target_dir !== path.join(fixture.workspace_root, ".tmp", "build-cache", "simulation-acceptance")
    || fixture.process_tmp_dir !== path.join(fixture.workspace_root, ".tmp", "process-tmp", "simulation-acceptance")) {
    fail("fixture binary workspace paths are invalid");
  }
  if (!Array.isArray(fixture.argv) || fixture.argv[0] !== fixture.executable_relative_path) fail("fixture binary argv is invalid");
}

function validateFixtureBuild(build, fixture) {
  requireExactKeys(build, ["argv", "cargo_build_jobs", "wall_ms", "executable_relative_path", "workspace_root", "cargo_target_dir", "process_tmp_dir", "binary_sha256", "binary_bytes"], "manifest.fixture_build");
  requireJsonEqual(build.argv, fixture.build_argv, "fixture build argv");
  if (build.cargo_build_jobs !== fixture.cargo_build_jobs) fail("fixture build job budget mismatch");
  requireNonNegativeDuration(build.wall_ms, "fixture build wall_ms");
  if (build.wall_ms > SIMULATION_BATCH_TIMEOUT_MS) fail("fixture build exceeded the five-minute hard maximum");
  if (build.executable_relative_path !== fixture.executable_relative_path
    || build.workspace_root !== fixture.workspace_root
    || build.cargo_target_dir !== fixture.cargo_target_dir
    || build.process_tmp_dir !== fixture.process_tmp_dir
    || build.binary_sha256 !== fixture.binary_sha256
    || build.binary_bytes !== fixture.binary_bytes) {
    fail("fixture build binary identity mismatch");
  }
}

function fixtureArgs(rootContext, spec, seed) {
  return [
    rootContext.fixtureBinary.executable_relative_path,
    spec.scenario, String(seed), String(spec.naturalDays), String(spec.behavior), String(spec.event), String(spec.volumeDenominatorAssumption),
  ];
}

function expectedIdentity(rootContext, spec) {
  return {
    runner: { schema: SIMULATION_CHECKPOINT_SCHEMA, schema_version: 4, runner_id: SIMULATION_RUNNER_ID, runner_policy_version: 8, effective_date: "2026-09-30", script: "scripts/simulation/baseline-run.mjs" },
    git: { revision: rootContext.git.revision, dirty_paths: rootContext.git.dirty_paths },
    source_fingerprint: rootContext.sourceFingerprint,
    resource_policy: rootContext.resourcePolicy,
    fixture: { ...rootContext.fixtureBinary, argv: fixtureArgs(rootContext, spec, "<seed>") },
    scenario: spec.scenario,
    ordered_seeds: [...spec.seeds],
    natural_days: spec.naturalDays,
    multipliers: { behavior: spec.behavior, event: spec.event, volume_denominator_assumption: spec.volumeDenominatorAssumption },
  };
}

function checkpointDigest(checkpoint) {
  return sha256(JSON.stringify({
    schema: checkpoint.schema, schema_version: checkpoint.schema_version,
    identity: checkpoint.identity,
    identity_digest: checkpoint.identity_digest,
    completed: checkpoint.completed,
  }));
}

function determinismReceiptDigest(receipt) {
  return sha256(JSON.stringify({
    schema: receipt.schema, schema_version: receipt.schema_version,
    identity: receipt.identity,
    identity_digest: receipt.identity_digest,
    seed: receipt.seed,
    first_digest: receipt.first_digest,
    rerun_digest: receipt.rerun_digest,
    identical: receipt.identical,
    argv: receipt.argv,
    exit_code: receipt.exit_code,
    wall_ms: receipt.wall_ms,
    revision: receipt.revision,
    source_fingerprint_digest: receipt.source_fingerprint_digest,
  }));
}

function validateSimulationRaw(raw, rootContext, spec, seed, label) {
  requireRecord(raw, label);
  if (raw.source !== "current_session_setup" || Object.hasOwn(raw, "save_path") || Object.hasOwn(raw, "load")) {
    fail(`${label} is not a fresh current simulation acceptance setup report`);
  }
  if (raw.build_source_fingerprint !== rootContext.sourceFingerprint.digest) fail(`${label} was emitted by a stale fixture binary`);
  if (raw.scenario !== spec.scenario || raw.seed !== String(seed) || raw.natural_days !== spec.naturalDays) fail(`${label} scenario, seed, or natural-day request mismatch`);
  requireJsonEqual(raw.multipliers, { behavior: spec.behavior, event: spec.event, volume_denominator_assumption: spec.volumeDenominatorAssumption }, `${label}.multipliers`);
  if (!isRecord(raw.calendar) || raw.calendar.natural_days !== spec.naturalDays
    || !Number.isInteger(raw.calendar.trading_days) || !Number.isInteger(raw.calendar.closed_days)
    || raw.calendar.trading_days + raw.calendar.closed_days !== spec.naturalDays) {
    fail(`${label} calendar accounting is incomplete`);
  }
  const profile = spec.scenario === "primary"
    ? { id: "primary-bounded-representative", retail: 64, ticks: 30, opening: 3, closing: 2, start: "2030-01-01" }
    : { id: "cross-year-bounded-representative", retail: 32, ticks: 20, opening: 3, closing: 2, start: "2030-12-27" };
  requireJsonEqual(raw.verification_profile, {
    schema: "simulation-bounded-representative-profile", schema_version: 1,
    profile_id: profile.id,
    scope: "bounded_representative_not_full_market_scale",
    retail_count: profile.retail,
    inst_count: 5,
    hot_count: 2,
    stock_count: 5,
    ticks_per_trading_day: profile.ticks,
    opening_auction_ticks: profile.opening,
    continuous_ticks: profile.ticks - profile.opening - profile.closing,
    closing_auction_ticks: profile.closing,
    start_date: profile.start,
    market_phases: ["opening_auction", "continuous", "closing_auction"],
  }, `${label}.verification_profile`);
  if (!Array.isArray(raw.price_volume?.runs) || raw.price_volume.runs.length !== 1 || raw.price_volume.runs[0]?.seed !== String(seed)) {
    fail(`${label} does not retain exactly one matching raw price-volume run`);
  }
  if (raw.price_volume.runs[0].retail_execution?.filled_share_ratio === null && raw.causal?.ratio_absent_reason !== "no_submissions") {
    fail(`${label} silently drops the reason for an absent execution ratio`);
  }
}

function entryWithoutRaw(run, label) {
  requireExactKeys(run, ["seed", "file", "sha256", "argv", "exit_code", "wall_ms", "revision", "dirty_paths", "source_fingerprint_digest", "ordered_seeds", "raw"], label);
  const { raw: _raw, ...entry } = run;
  return entry;
}

function validateEntry(entry, rootContext, spec, seed, label) {
  requireExactKeys(entry, ["seed", "file", "sha256", "argv", "exit_code", "wall_ms", "revision", "dirty_paths", "source_fingerprint_digest", "ordered_seeds"], label);
  if (entry.seed !== seed || entry.file !== `seed-${seed}.json`) fail(`${label} seed or raw filename mismatch`);
  requireSha256(entry.sha256, `${label}.sha256`);
  requireJsonEqual(entry.argv, fixtureArgs(rootContext, spec, seed), `${label}.argv`);
  if (entry.exit_code !== 0) fail(`${label}.exit_code must be zero`);
  requireNonNegativeDuration(entry.wall_ms, `${label}.wall_ms`);
  if (entry.revision !== rootContext.git.revision) fail(`${label}.revision mismatch`);
  requireJsonEqual(entry.dirty_paths, rootContext.git.dirty_paths, `${label}.dirty_paths`);
  if (entry.source_fingerprint_digest !== rootContext.sourceFingerprint.digest) fail(`${label}.source_fingerprint_digest mismatch`);
  requireJsonEqual(entry.ordered_seeds, spec.seeds, `${label}.ordered_seeds`);
}

function validateCheckpoint(checkpoint, identity, completed, label) {
  requireExactKeys(checkpoint, ["schema", "schema_version", "identity", "identity_digest", "completed", "checkpoint_digest"], label);
  if (checkpoint.schema !== SIMULATION_CHECKPOINT_SCHEMA || checkpoint.schema_version !== 4) fail(`${label} schema is unsupported`);
  requireJsonEqual(checkpoint.identity, identity, `${label}.identity`);
  const identityDigest = sha256(JSON.stringify(identity));
  if (checkpoint.identity_digest !== identityDigest) fail(`${label} identity digest mismatch`);
  requireJsonEqual(checkpoint.completed, completed, `${label}.completed`);
  if (checkpoint.checkpoint_digest !== checkpointDigest(checkpoint)) fail(`${label} checkpoint digest mismatch`);
}

async function verifyMatrix(root, report, rootContext, spec) {
  const directoryName = matrixDirectoryName(spec);
  const directoryPath = resolveContained(root, directoryName, `matrix ${directoryName}`);
  await requireDirectory(directoryPath, `matrix ${directoryName}`);
  const expectedArtifacts = ["checkpoint.json", "determinism.checkpoint.json"];
  for (const seed of spec.seeds) expectedArtifacts.push(`seed-${seed}.json`, `seed-${seed}.checkpoint.json`);
  await requireExactDirectoryEntries(directoryPath, expectedArtifacts, `matrix ${directoryName}`);

  requireExactKeys(report, ["scenario", "seeds", "natural_days", "multipliers", "source_fingerprint", "resource_policy", "identity", "runs", "complete", "finalized", "executed", "checkpoint", "quantiles_and_extremes", "determinism_check"], `manifest matrix ${directoryName}`);
  if (report.scenario !== spec.scenario || report.natural_days !== spec.naturalDays) fail(`manifest matrix ${directoryName} scenario or natural days mismatch`);
  requireJsonEqual(report.seeds, spec.seeds, `manifest matrix ${directoryName}.seeds`);
  requireJsonEqual(report.multipliers, { behavior: spec.behavior, event: spec.event, volume_denominator_assumption: spec.volumeDenominatorAssumption }, `manifest matrix ${directoryName}.multipliers`);
  requireJsonEqual(report.source_fingerprint, rootContext.sourceFingerprint, `manifest matrix ${directoryName}.source_fingerprint`);
  requireJsonEqual(report.resource_policy, rootContext.resourcePolicy, `manifest matrix ${directoryName}.resource_policy`);
  const identity = expectedIdentity(rootContext, spec);
  requireJsonEqual(report.identity, identity, `manifest matrix ${directoryName}.identity`);
  if (report.complete !== true || report.finalized !== true) fail(`manifest matrix ${directoryName} is not complete and finalized`);
  if (!Number.isInteger(report.executed) || report.executed < 0 || report.executed > spec.seeds.length) fail(`manifest matrix ${directoryName}.executed is malformed`);
  if (report.checkpoint !== `${directoryName}/checkpoint.json`) fail(`manifest matrix ${directoryName}.checkpoint path mismatch`);
  requireRecord(report.quantiles_and_extremes, `manifest matrix ${directoryName}.quantiles_and_extremes`);
  if (!Array.isArray(report.runs) || report.runs.length !== spec.seeds.length) fail(`manifest matrix ${directoryName} does not contain the complete seed matrix`);

  const completed = [];
  for (const [index, seed] of spec.seeds.entries()) {
    const run = report.runs[index];
    const label = `manifest matrix ${directoryName} seed ${seed}`;
    const entry = entryWithoutRaw(run, label);
    validateEntry(entry, rootContext, spec, seed, label);
    validateSimulationRaw(run.raw, rootContext, spec, seed, `${label}.raw`);
    const rawPath = resolveContained(directoryPath, entry.file, `${label}.file`);
    const rawArtifact = await readJson(rawPath, `${label} raw artifact`);
    if (sha256(rawArtifact.bytes) !== entry.sha256) fail(`${label} raw artifact digest mismatch`);
    requireJsonEqual(rawArtifact.parsed, run.raw, `${label} manifest/raw payload`);

    const seedCheckpointPath = resolveContained(directoryPath, `seed-${seed}.checkpoint.json`, `${label} receipt`);
    const seedCheckpoint = (await readJson(seedCheckpointPath, `${label} per-seed checkpoint`)).parsed;
    validateCheckpoint(seedCheckpoint, identity, [entry], `${label} per-seed checkpoint`);
    completed.push(entry);
  }

  const aggregate = (await readJson(resolveContained(directoryPath, "checkpoint.json", `matrix ${directoryName} aggregate`), `matrix ${directoryName} aggregate checkpoint`)).parsed;
  validateCheckpoint(aggregate, identity, completed, `matrix ${directoryName} aggregate checkpoint`);

  const canonical = completed.at(-1);
  const receipt = (await readJson(resolveContained(directoryPath, "determinism.checkpoint.json", `matrix ${directoryName} determinism receipt`), `matrix ${directoryName} determinism receipt`)).parsed;
  requireExactKeys(receipt, ["schema", "schema_version", "identity", "identity_digest", "seed", "first_digest", "rerun_digest", "identical", "argv", "exit_code", "wall_ms", "revision", "source_fingerprint_digest", "receipt_digest"], `matrix ${directoryName} determinism receipt`);
  if (receipt.schema !== SIMULATION_DETERMINISM_RECEIPT_SCHEMA || receipt.schema_version !== 1) fail(`matrix ${directoryName} determinism receipt schema is unsupported`);
  requireJsonEqual(receipt.identity, identity, `matrix ${directoryName} determinism receipt identity`);
  if (receipt.identity_digest !== sha256(JSON.stringify(identity))) fail(`matrix ${directoryName} determinism receipt identity digest mismatch`);
  if (receipt.seed !== canonical.seed || receipt.first_digest !== canonical.sha256 || receipt.rerun_digest !== canonical.sha256 || receipt.identical !== true) {
    fail(`matrix ${directoryName} determinism receipt is not bound to the canonical and rerun bytes`);
  }
  requireJsonEqual(receipt.argv, canonical.argv, `matrix ${directoryName} determinism receipt argv`);
  if (receipt.exit_code !== 0) fail(`matrix ${directoryName} determinism rerun exit code must be zero`);
  requireNonNegativeDuration(receipt.wall_ms, `matrix ${directoryName} determinism receipt wall_ms`);
  if (receipt.revision !== rootContext.git.revision || receipt.source_fingerprint_digest !== rootContext.sourceFingerprint.digest) {
    fail(`matrix ${directoryName} determinism receipt source provenance mismatch`);
  }
  if (receipt.receipt_digest !== determinismReceiptDigest(receipt)) fail(`matrix ${directoryName} determinism receipt digest mismatch`);
  requireExactKeys(report.determinism_check, ["seed", "first_digest", "rerun_digest", "identical", "revision", "receipt"], `manifest matrix ${directoryName}.determinism_check`);
  requireJsonEqual(report.determinism_check, {
    seed: receipt.seed,
    first_digest: receipt.first_digest,
    rerun_digest: receipt.rerun_digest,
    identical: true,
    revision: receipt.revision,
    receipt: "determinism.checkpoint.json",
  }, `manifest matrix ${directoryName}.determinism_check`);
  return { directoryName, canonicalRuns: spec.seeds.length, determinismReruns: 1 };
}

function afterSpecs() {
  return [
    { field: "primary", scenario: "primary", seeds: MATRIX_SEEDS, naturalDays: SIMULATION_PRIMARY_NATURAL_DAYS, behavior: 1, event: 1, volumeDenominatorAssumption: 1 },
    { field: "cross_year_four_industry", scenario: "cross-year", seeds: CROSS_YEAR_SEEDS, naturalDays: SIMULATION_CROSS_YEAR_NATURAL_DAYS, behavior: 1, event: 1, volumeDenominatorAssumption: 1 },
  ];
}

function sensitivityRequests() {
  return [
    ...SENSITIVITY_MULTIPLIERS.map((multiplier) => ({ dimension: "behavior", multiplier, scenario: "primary", seeds: MATRIX_SEEDS, naturalDays: SIMULATION_PRIMARY_NATURAL_DAYS, behavior: multiplier, event: 1, volumeDenominatorAssumption: 1 })),
    ...SENSITIVITY_MULTIPLIERS.map((multiplier) => ({ dimension: "event", multiplier, scenario: "primary", seeds: MATRIX_SEEDS, naturalDays: SIMULATION_PRIMARY_NATURAL_DAYS, behavior: 1, event: multiplier, volumeDenominatorAssumption: 1 })),
    ...SENSITIVITY_MULTIPLIERS.map((multiplier) => ({ dimension: "volume_denominator_assumption", multiplier, scenario: "primary", seeds: MATRIX_SEEDS, naturalDays: SIMULATION_PRIMARY_NATURAL_DAYS, behavior: 1, event: 1, volumeDenominatorAssumption: multiplier })),
  ];
}

function validateRootManifest(manifest) {
  requireRecord(manifest, "simulation acceptance manifest");
  if (manifest.command === "after") {
    requireExactKeys(manifest, ["command", "source", "git", "source_fingerprint", "fixture_binary", "fixture_build", "resource_policy", "primary", "cross_year_four_industry", "external_market_calibration_scope"], "after manifest");
  } else if (manifest.command === "sensitivity") {
    requireExactKeys(manifest, ["command", "source", "git", "source_fingerprint", "fixture_binary", "fixture_build", "resource_policy", "dimensions", "external_market_calibration_scope"], "sensitivity manifest");
  } else {
    fail(`simulation acceptance manifest command must be after or sensitivity, got ${JSON.stringify(manifest.command)}`);
  }
  if (manifest.source !== "current_session_setup") fail("simulation acceptance manifest source must be current_session_setup");
  if (manifest.external_market_calibration_scope !== "not_applicable_synthetic_history_only") {
    fail("simulation acceptance manifest must mark external market calibration not applicable under the synthetic-history policy");
  }
  validateGit(manifest.git);
  validateSourceFingerprint(manifest.source_fingerprint);
  validateResourcePolicy(manifest.resource_policy);
  validateFixtureBinary(manifest.fixture_binary, manifest.resource_policy);
  if (manifest.fixture_binary.embedded_source_fingerprint_digest !== manifest.source_fingerprint.digest) {
    fail("fixture binary is stale for the sealed source fingerprint");
  }
  validateFixtureBuild(manifest.fixture_build, manifest.fixture_binary);
}

export async function verifySimulationArtifacts(rootPath) {
  if (typeof rootPath !== "string" || rootPath.length === 0) fail("simulation acceptance root path is required");
  const root = path.resolve(rootPath);
  await requireDirectory(root, "simulation acceptance root");
  const manifest = (await readJson(path.join(root, "manifest.json"), "simulation acceptance root manifest")).parsed;
  validateRootManifest(manifest);
  const rootContext = { git: manifest.git, sourceFingerprint: manifest.source_fingerprint, fixtureBinary: manifest.fixture_binary, resourcePolicy: manifest.resource_policy };
  const expectedRootEntries = ["manifest.json"];
  let canonicalRuns = 0;
  let determinismReruns = 0;
  let matrices = 0;

  if (manifest.command === "after") {
    for (const spec of afterSpecs()) {
      expectedRootEntries.push(matrixDirectoryName(spec));
      const result = await verifyMatrix(root, manifest[spec.field], rootContext, spec);
      canonicalRuns += result.canonicalRuns;
      determinismReruns += result.determinismReruns;
      matrices += 1;
    }
  } else {
    const requests = sensitivityRequests();
    if (!Array.isArray(manifest.dimensions) || manifest.dimensions.length !== requests.length) fail("sensitivity manifest must contain the exact nine dimension rows");
    const reports = new Map();
    for (const [index, request] of requests.entries()) {
      const dimension = manifest.dimensions[index];
      requireExactKeys(dimension, ["dimension", "multiplier", "reuse", "report"], `sensitivity dimension ${index}`);
      if (dimension.dimension !== request.dimension || dimension.multiplier !== request.multiplier) fail(`sensitivity dimension ${index} order or multiplier mismatch`);
      const key = `${request.behavior}/${request.event}/${request.volumeDenominatorAssumption}`;
      const previous = reports.get(key);
      if (previous === undefined) {
        requireJsonEqual(dimension.reuse, { executed_or_resumed: true }, `sensitivity dimension ${index}.reuse`);
        reports.set(key, dimension.report);
        expectedRootEntries.push(matrixDirectoryName(request));
        const result = await verifyMatrix(root, dimension.report, rootContext, request);
        canonicalRuns += result.canonicalRuns;
        determinismReruns += result.determinismReruns;
        matrices += 1;
      } else {
        requireJsonEqual(dimension.reuse, { canonical_spec: key, validated: true }, `sensitivity dimension ${index}.reuse`);
        requireJsonEqual(dimension.report, previous, `sensitivity dimension ${index} reused report`);
      }
    }
  }

  await requireExactDirectoryEntries(root, expectedRootEntries, "simulation acceptance root");
  return {
    command: manifest.command,
    matrices,
    canonical_runs: canonicalRuns,
    determinism_reruns: determinismReruns,
    total_executions: canonicalRuns + determinismReruns,
    source_fingerprint_digest: manifest.source_fingerprint.digest,
  };
}

export async function main(argv) {
  if (argv.length !== 1 || argv[0].startsWith("-")) {
    fail("usage: node scripts/simulation/verify-simulation-artifacts.mjs <after-or-sensitivity-root>");
  }
  const summary = await verifySimulationArtifacts(argv[0]);
  writeSync(process.stdout.fd, `${JSON.stringify({ status: "verified", ...summary })}\n`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch((error) => {
    writeSync(process.stderr.fd, `simulation acceptance root verification failed: ${error.message}\n`);
    process.exitCode = 1;
  });
}
