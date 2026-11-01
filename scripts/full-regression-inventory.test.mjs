import assert from "node:assert/strict";

import { readFile, readdir, rm, writeFile } from "node:fs/promises";
import { it } from "node:test";

import path from "node:path";

import { buildFullRegressionArtifacts, executeFullRegression } from "./run-full-regression.mjs";
import { resolveWorkspaceRoot } from "./workspace-paths.mjs";
import { runBoundedCommand } from "./run-with-deadline.mjs";
import * as regression from "./run-full-regression.mjs";

import { CARGO_BUILD_JOBS_ENV, createSealedFixture, resealInventory } from "./full-regression-test-support.mjs";

it("fake build/execute 的 Cargo jobs 与所有预算日志一致且保留执行并发", { timeout: 10000 }, async () => {
  const logs = [];
  const baseEnv = { ...process.env, [CARGO_BUILD_JOBS_ENV]: "8" };
  const fixture = await createSealedFixture({ buildOptions: {
    cpuCount: 128, baseEnv,
    progressIntervalMs: 5,
    log: (line) => logs.push(JSON.parse(line)),
    run: async (options, stdout) => {
      assert.equal(options.command, "cargo");
      assert.equal(options.env.CARGO_BUILD_JOBS, "8");
      await new Promise((resolve) => setTimeout(resolve, 20));
      return { stdout, stderr: "" };
    },
  } });
  try {
    const start = logs.find((entry) => entry.phase === "build-start");
    assert.equal(start.cargo_build_jobs, 8);
    assert.equal(start.available_cpu_count, 128);
    const budgetLogs = logs.filter((entry) => Object.hasOwn(entry, "cpu_jobs"));
    assert.ok(budgetLogs.some((entry) => entry.stage === "source_fingerprint"));
    assert.ok(budgetLogs.some((entry) => entry.status === "running"));
    assert.ok(budgetLogs.every((entry) => entry.cpu_jobs === 8));
    const inventory = JSON.parse(await readFile(fixture.inventoryPath, "utf8"));
    assert.equal(inventory.build.available_cpu_count, 128);
    let scriptsObserved = false;
    const result = await executeFullRegression({
      cpuCount: 128, baseEnv, inventoryPath: fixture.inventoryPath,
      collectFingerprint: async () => fixture.fingerprint,
      runScripts: async ({ env, cpuCount }) => {
        assert.equal(env[CARGO_BUILD_JOBS_ENV], "8");
        assert.equal(cpuCount, 32);
        scriptsObserved = true;
        return { file_count: 1 };
      },
      run: async ({ command, args, env }) => {
        assert.equal(env[CARGO_BUILD_JOBS_ENV], "8");
        if (command === "cargo") {
          assert.equal(env.CARGO_BUILD_JOBS, "8");
          assert.equal(args.at(-1), "--test-threads=95");
        }
        return { stdout: args.includes("--list")
          ? (args.includes("--exact")
            ? "lifecycle::year_boundary_keeps_company_operations_and_disclosure_state_authoritative: test\n"
            : (args.includes("--ignored") ? "" : Array.from({ length: 80 }, (_, index) => `ordinary::case${index}: test\n`).join("")))
          : "" };
      },
    });
    const binaries = result.steps.find((step) => step.label === "Rust prebuilt test binaries");
    assert.equal(binaries.available_cpu_count, 96);
    assert.equal(binaries.max_concurrent_cases, 32);
    assert.equal(binaries.test_threads, 1);
    assert.equal(scriptsObserved, true);
  } finally { await fixture.cleanup(); }
});

it("reports Cargo build progress before a real timed-out child is killed", async () => {
  const workspaceRoot = await resolveWorkspaceRoot(process.cwd());
  const inventoryPath = path.join(workspaceRoot, ".tmp", "full-regression", `timeout-inventory-${process.pid}-${Date.now()}.json`);
  const logs = [];
  const diagnostics = [];
  const message = JSON.stringify({ reason: "compiler-artifact", package_id: "slow-package 0.1.0", target: { name: "slow_test" } });
  try {
    await assert.rejects(buildFullRegressionArtifacts({
      cwd: process.cwd(), inventoryPath,
      cpuCount: 4, baseEnv: {},
      collectFingerprint: async () => ({ algorithm: "test", algorithm_version: 1, files: [], digest: "a".repeat(64) }),
      log: (line) => logs.push(JSON.parse(line)),
      writeDiagnostic: (chunk) => diagnostics.push(chunk.toString()),
      progressIntervalMs: 100,
      run: (options) => runBoundedCommand({
        ...options,
        command: process.execPath,
        args: ["-e", `process.stdout.write(${JSON.stringify(`${message}\n`)}); process.stderr.write('Compiling slow-package\\n'); setInterval(() => {}, 1000)`],
        timeoutMs: 650,
        cleanupReserveMs: 100,
      }),
    }), /total 650ms deadline.*process tree/i);
    assert.ok(logs.some((entry) => entry.phase === "build" && entry.status === "started" && entry.cpu_jobs === 4));
    assert.ok(logs.some((entry) => entry.phase === "build" && entry.status === "running" && entry.last_completed_target === "slow-package 0.1.0 slow_test" && entry.elapsed_ms >= 100));
    assert.ok(logs.some((entry) => entry.phase === "build" && entry.status === "failed" && entry.last_completed_target === "slow-package 0.1.0 slow_test"));
    assert.match(diagnostics.join(""), /Compiling slow-package/);
    await assert.rejects(readFile(inventoryPath), /ENOENT/);
  } finally {
    await rm(inventoryPath, { force: true });
  }
});

it("reports bounded rendered Cargo errors from a real failing child", async () => {
  const workspaceRoot = await resolveWorkspaceRoot(process.cwd());
  const inventoryPath = path.join(workspaceRoot, ".tmp", "full-regression", `failure-inventory-${process.pid}-${Date.now()}.json`);
  const logs = [];
  const message = JSON.stringify({ reason: "compiler-message", package_id: "broken-package 0.1.0", message: { level: "error", rendered: "error[E0001]: deliberate fixture failure" } });
  try {
    await assert.rejects(buildFullRegressionArtifacts({
      cwd: process.cwd(), inventoryPath,
      collectFingerprint: async () => ({ algorithm: "test", algorithm_version: 1, files: [], digest: "a".repeat(64) }),
      log: (line) => logs.push(JSON.parse(line)),
      writeDiagnostic: () => undefined,
      run: (options) => runBoundedCommand({
        ...options,
        command: process.execPath,
        args: ["-e", `process.stdout.write(${JSON.stringify(message)}); process.exit(2)`],
      }),
    }), /exited with 2/i);
    assert.ok(logs.some((entry) => entry.status === "compiler_error" && entry.package === "broken-package 0.1.0" && /deliberate fixture failure/.test(entry.diagnostic)));
    assert.ok(logs.some((entry) => entry.status === "failed" && entry.compiler_errors === 1));
    await assert.rejects(readFile(inventoryPath), /ENOENT/);
  } finally {
    await rm(inventoryPath, { force: true });
  }
});

it("keeps the timeout reason when Cargo leaves an incomplete JSON line", async () => {
  const workspaceRoot = await resolveWorkspaceRoot(process.cwd());
  const inventoryPath = path.join(workspaceRoot, ".tmp", "full-regression", `partial-inventory-${process.pid}-${Date.now()}.json`);
  const logs = [];
  try {
    await assert.rejects(buildFullRegressionArtifacts({
      cwd: process.cwd(), inventoryPath,
      collectFingerprint: async () => ({ algorithm: "test", algorithm_version: 1, files: [], digest: "a".repeat(64) }),
      log: (line) => logs.push(JSON.parse(line)),
      writeDiagnostic: () => undefined,
      run: (options) => runBoundedCommand({
        ...options,
        command: process.execPath,
        args: ["-e", `process.stdout.write(${JSON.stringify('{"reason":')}); setInterval(() => {}, 1000)`],
        timeoutMs: 500,
        cleanupReserveMs: 100,
      }),
    }), (error) => /total 500ms deadline/i.test(error.message) && /malformed JSON/i.test(error.message));
    assert.ok(logs.some((entry) => entry.status === "failed" && /total 500ms deadline/i.test(entry.error) && /malformed JSON/i.test(entry.error)));
    await assert.rejects(readFile(inventoryPath), /ENOENT/);
  } finally {
    await rm(inventoryPath, { force: true });
  }
});

it("seals the source identity and exact binary bytes in an atomic build inventory", async () => {
  const fixture = await createSealedFixture();
  try {
    const inventory = JSON.parse(await readFile(fixture.inventoryPath, "utf8"));
    assert.equal(inventory.schema, "full-regression-artifact-inventory");
    assert.equal(inventory.schema_version, 1);
    assert.equal(inventory.source_fingerprint.digest, fixture.fingerprint.digest);
    assert.match(inventory.identity_digest, /^[a-f0-9]{64}$/);
    assert.equal(inventory.artifacts.length, 1);
    assert.match(inventory.artifacts[0].executable_relative_path, /^\.tmp\/build-cache\/full-regression\//);
    assert.equal(inventory.artifacts[0].bytes, Buffer.byteLength("sealed test binary"));
    assert.match(inventory.artifacts[0].sha256, /^[a-f0-9]{64}$/);
    const temporaryFiles = (await readdir(path.dirname(fixture.inventoryPath)))
      .filter((entry) => entry.startsWith(`${path.basename(fixture.inventoryPath)}.`) && entry.endsWith(".tmp"));
    assert.deepEqual(temporaryFiles, []);
  } finally {
    await fixture.cleanup();
  }
});

it("owns an immutable inventory while preserving decoded order, extras and duplicate artifacts", async () => {
  assert.equal(typeof regression.ArtifactInventory, "function", "ArtifactInventory must own the sealed record");
  const fixture = await createSealedFixture();
  try {
    const decoded = JSON.parse(await readFile(fixture.inventoryPath, "utf8"));
    const reordered = resealInventory({
      extra: { note: "保留扩展字段" },
      artifacts: [decoded.artifacts[0], decoded.artifacts[0]],
      ...Object.fromEntries(Object.entries(decoded).filter(([key]) => key !== "artifacts")),
    });
    const expectedIdentity = {
      sourceRoot: decoded.source_root,
      workspacePaths: {
        workspaceRoot: decoded.workspace_root,
        cargoTargetDir: decoded.cargo_target_dir,
        processTmpDir: decoded.process_tmp_dir,
      },
      fingerprint: fixture.fingerprint,
    };
    const inventory = regression.ArtifactInventory.fromDecodedInventory(reordered, expectedIdentity);
    const originalJson = JSON.stringify(reordered);
    reordered.extra.note = "调用方修改";
    reordered.artifacts[0].bytes += 1;
    assert.equal(JSON.stringify(inventory.toJson()), originalJson);
    const exported = inventory.toJson();
    exported.artifacts.pop();
    exported.extra.note = "输出修改";
    assert.equal(JSON.stringify(inventory.toJson()), originalJson);
    assert.equal(inventory.artifactDescriptors().length, 2);
    assert.throws(() => inventory.artifactDescriptors().pop(), TypeError);
    assert.throws(() => { inventory.artifactDescriptors()[0].bytes += 1; }, TypeError);
    const staleOrder = { ...decoded };
    delete staleOrder.schema;
    staleOrder.schema = decoded.schema;
    assert.throws(() => regression.ArtifactInventory.fromDecodedInventory(staleOrder, expectedIdentity), /malformed, stale, or bound/i);
    assert.equal(JSON.stringify(regression.ArtifactInventory.fromDecodedInventory(
      resealInventory(staleOrder), expectedIdentity,
    ).toJson()), JSON.stringify(resealInventory(staleOrder)));
  } finally {
    await fixture.cleanup();
  }
});

it("accepts a correctly sealed reordered inventory with extras and duplicate artifacts during execution", async () => {
  const fixture = await createSealedFixture();
  try {
    const decoded = JSON.parse(await readFile(fixture.inventoryPath, "utf8"));
    const reordered = resealInventory({ extra: "保留", ...decoded, artifacts: [...decoded.artifacts, ...decoded.artifacts] });
    await writeFile(fixture.inventoryPath, JSON.stringify(reordered));
    let ordinaryExecutions = 0;
    await assert.rejects(executeFullRegression({
      cpuCount: 1,
      inventoryPath: fixture.inventoryPath,
      collectFingerprint: async () => fixture.fingerprint,
      run: async ({ command, args }) => {
        assert.equal(command, fixture.artifact);
        if (args.includes("--list")) return { stdout: args.includes("--ignored") ? "" : "ordinary: test\n" };
        ordinaryExecutions += 1;
      },
    }), /required long validation target.*resolved to 2 prebuilt binaries/i);
    assert.equal(ordinaryExecutions, 2, "重复 artifacts 必须保留现有执行语义");
  } finally {
    await fixture.cleanup();
  }
});

it("preserves deeply nested JSON extras accepted by the inventory wire format", async () => {
  const fixture = await createSealedFixture();
  try {
    const decoded = JSON.parse(await readFile(fixture.inventoryPath, "utf8"));
    let extra = { note: "保留深层扩展", empty: null };
    for (let depth = 0; depth < 2000; depth += 1) extra = { nested: extra, empty: null };
    const record = resealInventory({ ...decoded, extra });
    const originalJson = JSON.stringify(record);
    const callerRecord = JSON.parse(originalJson);
    const inventory = regression.ArtifactInventory.fromDecodedInventory(callerRecord, {
      sourceRoot: decoded.source_root,
      workspacePaths: {
        workspaceRoot: decoded.workspace_root,
        cargoTargetDir: decoded.cargo_target_dir,
        processTmpDir: decoded.process_tmp_dir,
      },
      fingerprint: fixture.fingerprint,
    });
    assert.equal(JSON.stringify(inventory.toJson()), originalJson);
    let callerExtra = callerRecord.extra;
    let exportedExtra = inventory.toJson().extra;
    for (let depth = 0; depth < 2000; depth += 1) {
      assert.equal(exportedExtra.empty, null);
      callerExtra = callerExtra.nested;
      exportedExtra = exportedExtra.nested;
    }
    callerExtra.note = "修改输入叶子";
    exportedExtra.note = "修改输出叶子";
    assert.equal(JSON.stringify(inventory.toJson()), originalJson);
    await writeFile(fixture.inventoryPath, originalJson);
    let calls = 0;
    await executeFullRegression({
      runScripts: async () => ({ file_count: 0 }),
      inventoryPath: fixture.inventoryPath,
      collectFingerprint: async () => fixture.fingerprint,
      run: async ({ args }) => {
        calls += 1;
        return { stdout: args.includes("--list")
          ? (args.includes("--exact") ? "lifecycle::year_boundary_keeps_company_operations_and_disclosure_state_authoritative: test\n" : (args.includes("--ignored") ? "" : "ordinary: test\n"))
          : "" };
      },
    });
    assert.equal(calls, 18);
  } finally {
    await fixture.cleanup();
  }
});

it("rejects missing identity fields and changed seals before starting any test", async () => {
  const fixture = await createSealedFixture();
  let runs = 0;
  try {
    const original = JSON.parse(await readFile(fixture.inventoryPath, "utf8"));
    const mutations = [
      ["missing schema", (record) => { delete record.schema; }, /malformed, stale, or bound/i],
      ["legacy schema", (record) => { record.schema = "full-regression-artifact-inventory-v1"; }, /malformed, stale, or bound/i],
      ["missing schema version", (record) => { delete record.schema_version; }, /malformed, stale, or bound/i],
      ["string schema version", (record) => { record.schema_version = "1"; }, /malformed, stale, or bound/i],
      ["unknown schema version", (record) => { record.schema_version = 2; }, /malformed, stale, or bound/i],
      ["missing digest", (record) => { delete record.identity_digest; }, /malformed, stale, or bound/i, false],
      ["changed source", (record) => { record.source_root += "-other"; }, /malformed, stale, or bound/i],
      ["changed workspace", (record) => { record.workspace_root += "-other"; }, /malformed, stale, or bound/i],
      ["changed target", (record) => { record.cargo_target_dir += "-other"; }, /malformed, stale, or bound/i],
      ["changed temp", (record) => { record.process_tmp_dir += "-other"; }, /malformed, stale, or bound/i],
      ["changed fingerprint", (record) => { record.source_fingerprint.digest = "b".repeat(64); }, /malformed, stale, or bound/i],
      ["changed fingerprint algorithm", (record) => { record.source_fingerprint.algorithm = "full-regression-source-v1"; }, /malformed, stale, or bound/i],
      ["missing fingerprint algorithm version", (record) => { delete record.source_fingerprint.algorithm_version; }, /malformed, stale, or bound/i],
      ["changed fingerprint algorithm version", (record) => { record.source_fingerprint.algorithm_version = 2; }, /malformed, stale, or bound/i],
      ["missing artifacts", (record) => { delete record.artifacts; }, /malformed, stale, or bound/i],
      ["empty artifacts", (record) => { record.artifacts = []; }, /malformed, stale, or bound/i],
      ["missing artifact label", (record) => { delete record.artifacts[0].label; }, /invalid artifact entry/i],
      ["invalid artifact bytes", (record) => { record.artifacts[0].bytes = -1; }, /invalid artifact entry/i],
      ["invalid artifact digest", (record) => { record.artifacts[0].sha256 = "invalid"; }, /invalid artifact entry/i],
      ["absolute artifact path", (record) => { record.artifacts[0].executable_relative_path = fixture.artifact; }, /invalid artifact entry/i],
      ["escaped artifact path", (record) => { record.artifacts[0].executable_relative_path = "../escaped"; }, /escaped the Cargo target/i],
      ["changed artifact bytes", (record) => { record.artifacts[0].bytes += 1; }, /does not match its sealed inventory/i],
      ["changed artifact hash", (record) => { record.artifacts[0].sha256 = "0".repeat(64); }, /SHA-256 mismatch/i],
      ["unsealed extra field", (record) => { record.extra = "修改"; }, /malformed, stale, or bound/i, false],
      ["unsealed field order", (record) => { delete record.schema; record.schema = original.schema; }, /malformed, stale, or bound/i, false],
    ];
    for (const [label, mutate, errorPattern, reseal = true] of mutations) {
      const record = structuredClone(original);
      mutate(record);
      await writeFile(fixture.inventoryPath, JSON.stringify(reseal ? resealInventory(record) : record));
      await assert.rejects(executeFullRegression({
        inventoryPath: fixture.inventoryPath,
        collectFingerprint: async () => fixture.fingerprint,
        run: async () => { runs += 1; },
      }), errorPattern, label);
      assert.equal(runs, 0, label);
    }
  } finally {
    await fixture.cleanup();
  }
});
