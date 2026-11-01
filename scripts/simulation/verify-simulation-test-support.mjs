import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFile } from "node:child_process";
import { cp, mkdtemp, readFile, rm, unlink, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { after, before } from "node:test";

import {
  buildSimulationResourcePolicy,
  captureAfter as captureAfterWithPreparedFixture,
  captureSensitivity as captureSensitivityWithPreparedFixture,
} from "./baseline-run.mjs";
import { prepareWorkspacePaths } from "../workspace-paths.mjs";

export const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const TEST_REVISION = "6ad461e7f735ee1a0b4090497c58380af3422761";
const TEST_TREE = "d6e7c4a051f231a789ef589c4ec33580ad082109";
export const RESOURCE_POLICY = buildSimulationResourcePolicy(2, "verification_test");
const FAKE_SIMULATION_EXECUTABLE = ".tmp/build-cache/simulation-acceptance/release/examples/simulation_baseline_fixture";
const tempDirs = [];
export let validAfterRoot;
export let validSensitivityRoot;
const TEST_WORKSPACE_PATHS = await prepareWorkspacePaths({ sourceRoot: REPO_ROOT, scope: "simulation-acceptance" });

async function fakePrepareFixture({ sourceFingerprint, workspacePaths }) {
  return {
    executable_path: path.join(workspacePaths.workspaceRoot, ...FAKE_SIMULATION_EXECUTABLE.split("/")),
    executable_relative_path: FAKE_SIMULATION_EXECUTABLE,
    binary_sha256: "b".repeat(64),
    binary_bytes: 2048,
    embedded_source_fingerprint_digest: sourceFingerprint.digest,
    build_argv: ["cargo", "build", "-p", "engine", "--release", "--features", "simulation-diagnostics", "--example", "simulation_baseline_fixture", "--message-format=json-render-diagnostics"],
    cargo_build_jobs: RESOURCE_POLICY.available_cpu_count,
    workspace_root: workspacePaths.workspaceRoot,
    cargo_target_dir: workspacePaths.cargoTargetDir,
    process_tmp_dir: workspacePaths.processTmpDir,
    build_wall_ms: 1,
  };
}

export function captureAfter(options) {
  return captureAfterWithPreparedFixture({ prepareFixture: fakePrepareFixture, workspacePaths: TEST_WORKSPACE_PATHS, ...options });
}

function captureSensitivity(options) {
  return captureSensitivityWithPreparedFixture({ prepareFixture: fakePrepareFixture, workspacePaths: TEST_WORKSPACE_PATHS, ...options });
}

export async function newTempDir(prefix = "verify-simulation-artifacts-test-") {
  assert.ok(path.isAbsolute(process.env.TMPDIR ?? ""), "TMPDIR must be an absolute workspace path");
  const directory = await mkdtemp(path.join(process.env.TMPDIR, prefix));
  tempDirs.push(directory);
  return directory;
}

export function fakeSimulationExec() {
  return async (file, args, options) => {
    if (file === "git" && args[0] === "rev-parse" && args[1] === "HEAD") return { code: 0, stdout: `${TEST_REVISION}\n`, stderr: "" };
    if (file === "git" && args[0] === "rev-parse" && args[1] === "HEAD^{tree}") return { code: 0, stdout: `${TEST_TREE}\n`, stderr: "" };
    if (file === "git" && args[0] === "branch") return { code: 0, stdout: "verification-fixture\n", stderr: "" };
    if (file === "git" && (args[0] === "status" || args[0] === "diff" || args[0] === "ls-files")) return { code: 0, stdout: "", stderr: "" };
    if (file === FAKE_SIMULATION_EXECUTABLE || file.endsWith(`/${FAKE_SIMULATION_EXECUTABLE}`)) {
      const scenario = args[0];
      const seed = args[1];
      const naturalDays = Number(args[2]);
      const behavior = Number(args[3]);
      const event = Number(args[4]);
      const volumeDenominatorAssumption = Number(args[5]);
      const crossYear = scenario === "cross-year";
      const ticks = crossYear ? 20 : 30;
      return {
        code: 0,
        stdout: JSON.stringify({
          tool: "simulation_baseline_fixture",
          source: "current_session_setup",
          build_source_fingerprint: options.env.SIMULATION_SOURCE_FINGERPRINT_DIGEST,
          scenario,
          seed,
          natural_days: naturalDays,
          calendar: { natural_days: naturalDays, trading_days: naturalDays, closed_days: 0, policy_id: "a-share-simulation" },
          verification_profile: {
            schema: "simulation-bounded-representative-profile", schema_version: 1,
            profile_id: `${scenario}-bounded-representative`,
            scope: "bounded_representative_not_full_market_scale",
            retail_count: crossYear ? 32 : 64,
            inst_count: 5,
            hot_count: 2,
            stock_count: 5,
            ticks_per_trading_day: ticks,
            opening_auction_ticks: 3,
            continuous_ticks: ticks - 5,
            closing_auction_ticks: 2,
            start_date: crossYear ? "2030-12-27" : "2030-01-01",
            market_phases: ["opening_auction", "continuous", "closing_auction"],
          },
          // Rust's serde_json map order is not a wire-contract requirement.
          // Keep this deliberately different from the verifier's expected key
          // construction order so semantic object equality is exercised.
          multipliers: { behavior, volume_denominator_assumption: volumeDenominatorAssumption, event },
          price_volume: { runs: [{ seed, stocks: {}, retail_execution: { filled_share_ratio: 1 } }], stocks: {} },
          causal: { ratio_absent_reason: null },
        }),
        stderr: "",
      };
    }
    throw new Error(`unexpected fixture command: ${file} ${args.join(" ")}`);
  };
}

export function runNode(args, cwd = REPO_ROOT, timeoutMs = 1_000) {
  return new Promise((resolve) => {
    execFile(process.execPath, args, { cwd, timeout: timeoutMs }, (error, stdout, stderr) => {
      if (error === null) {
        resolve({ code: 0, killed: false, signal: null, stdout, stderr });
      } else {
        resolve({ code: error.code, killed: error.killed, signal: error.signal, stdout, stderr });
      }
    });
  });
}

export async function cloneRoot(source, name) {
  const parent = await newTempDir();
  const target = path.join(parent, name);
  await cp(source, target, { recursive: true });
  return target;
}

export function installVerifierFixture({ sensitivity = false } = {}) {
  before(async () => {
    const fixtureParent = await newTempDir("verify-simulation-artifacts-valid-");
    validAfterRoot = path.join(fixtureParent, "after");
    const exec = fakeSimulationExec();
    await captureAfter({ outputDir: validAfterRoot, exec, repoRoot: REPO_ROOT, resourcePolicy: RESOURCE_POLICY });
    if (sensitivity) {
      validSensitivityRoot = path.join(fixtureParent, "sensitivity");
      await captureSensitivity({ outputDir: validSensitivityRoot, exec, repoRoot: REPO_ROOT, resourcePolicy: RESOURCE_POLICY });
    }
  });
  after(async () => {
    await Promise.all(tempDirs.map((directory) => rm(directory, { force: true, recursive: true })));
  });
}
