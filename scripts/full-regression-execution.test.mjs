import assert from "node:assert/strict";

import { chmod, mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { it } from "node:test";
import { Worker } from "node:worker_threads";
import { hashArtifactFiles } from "./full-regression-artifact-hash.mjs";

import path from "node:path";

import { buildFullRegressionArtifacts, executeFullRegression, fullRegressionSteps, main, parseFullRegressionArgs, runFullRegression, runFullRegressionPhase } from "./run-full-regression.mjs";
import { prepareWorkspacePaths, resolveWorkspaceRoot } from "./workspace-paths.mjs";

import * as regression from "./run-full-regression.mjs";
import { discoverScriptTestFiles } from "./run-script-tests.mjs";

import { createSealedFixture, resealInventory, createExternalTargetFixture } from "./full-regression-test-support.mjs";

it("executes sealed prebuilt test binaries, then preserves doctests and 10-second web coverage", async () => {
  let nowMs = 0;
  const calls = [];
  const fixture = await createSealedFixture();
  try {
    const result = await executeFullRegression({
      cwd: process.cwd(),
      inventoryPath: fixture.inventoryPath,
      cpuCount: 8, baseEnv: {},
      now: () => nowMs,
      collectFingerprint: async () => fixture.fingerprint,
      run: async (options) => {
        calls.push(options);
        nowMs += 1;
        return { stdout: options.args.includes("--list")
          ? (options.args.includes("--exact")
            ? "lifecycle::year_boundary_keeps_company_operations_and_disclosure_state_authoritative: test\n"
            : (options.args.includes("--ignored") ? "" : "ordinary::case: test\n"))
          : "", stderr: "" };
      },
    });
    assert.equal(calls.filter((call) => call.command === fixture.artifact).length, 5);
    const ordinaryBinary = calls.find((call) => call.args.includes("ordinary::case"));
    assert.equal(ordinaryBinary.timeoutMs, 10_000);
    assert.deepEqual(ordinaryBinary.args, ["ordinary::case", "--exact", "--test-threads=1"]);
    // 批 9 A7：RAYON_NUM_THREADS 导出退役——断言缺省。
    assert.equal(ordinaryBinary.env.RAYON_NUM_THREADS, undefined);
    const longValidationListing = calls.find((call) => call.command === fixture.artifact && call.args.includes("--list") && call.args.includes("--exact"));
    assert.deepEqual(longValidationListing.args, [
      "--list",
      "--format=terse",
      "--exact",
      "--ignored",
      "lifecycle::year_boundary_keeps_company_operations_and_disclosure_state_authoritative",
    ]);
    const longValidation = calls.find((call) => call.command === fixture.artifact && !call.args.includes("--list") && call.args.includes("--ignored"));
    assert.deepEqual(longValidation.args, [
      "lifecycle::year_boundary_keeps_company_operations_and_disclosure_state_authoritative",
      "--exact",
      "--ignored",
      "--test-threads=1",
    ]);
    assert.equal(longValidation.env.RAYON_NUM_THREADS, undefined);
    assert.equal(calls.filter((call) => call.command === "cargo" && call.args.includes("--doc")).length, 1);
    assert.equal(calls.find((call) => call.command === "cargo" && call.args.includes("--doc")).timeoutMs, 299_984);
    const web = calls.filter((call) => call.args?.some((argument) => argument.endsWith("scripts/run-web-tests.mjs")));
    assert.deepEqual(web.map((call) => call.args.slice(-2)), Array.from({ length: 12 }, (_, index) => [String(index), "12"]));
    assert.ok(web.every((call) => call.command === process.execPath && call.timeoutMs <= 10_000));
    const scripts = calls.filter((call) => call.args?.some((argument) => argument.endsWith(".test.mjs")));
    assert.deepEqual(scripts.map((call) => call.args.at(-1)).sort(), await discoverScriptTestFiles(process.cwd()));
    assert.ok(scripts.every((call) => call.timeoutMs <= 10_000 && call.args.includes("--test-timeout=10000")));
    assert.equal(new Set(scripts.map((call) => call.args.at(-1))).size, scripts.length);
    assert.equal(result.steps.find((step) => step.label === "scripts ordinary tests").file_count, scripts.length);
    assert.equal(result.steps.find((step) => step.label === "Rust prebuilt test binaries").binary_count, 1);
    assert.equal(result.steps.find((step) => step.label === "Rust required long validations").case_count, 1);
  } finally {
    await fixture.cleanup();
  }
});

it("第九个 Web 批次失败立即终止后续测试并服从共享剩余期限", async () => {
  const fixture = await createSealedFixture();
  let nowMs = 0;
  const timeouts = [];
  let scriptsCalled = false;
  try {
    await assert.rejects(executeFullRegression({
      inventoryPath: fixture.inventoryPath, cpuCount: 8, now: () => nowMs,
      collectFingerprint: async () => fixture.fingerprint,
      runScripts: async () => { scriptsCalled = true; },
      run: async (options) => {
        if (options.args.some((arg) => arg.endsWith("scripts/run-web-tests.mjs"))) {
          timeouts.push(options.timeoutMs);
          nowMs += 37_000;
          if (timeouts.length === 9) throw new Error("第九批失败");
        }
        return { stdout: options.args.includes("--list")
          ? (options.args.includes("--exact") ? "lifecycle::year_boundary_keeps_company_operations_and_disclosure_state_authoritative: test\n" : (options.args.includes("--ignored") ? "" : "ordinary: test\n")) : "" };
      },
    }), /第九批失败/);
    assert.deepEqual(timeouts, [10000, 10000, 10000, 10000, 10000, 10000, 10000, 10000, 4000]);
    assert.equal(scriptsCalled, false);
  } finally { await fixture.cleanup(); }
});

it("fails fast and aborts in-flight sibling test binaries", async () => {
  const workspaceRoot = await resolveWorkspaceRoot(process.cwd());
  const target = path.join(workspaceRoot, ".tmp", "build-cache", "full-regression");
  const artifactPaths = [0, 1].map((index) => path.join(target, "debug", "deps", `failure-fixture-${process.pid}-${index}`));
  await mkdir(path.dirname(artifactPaths[0]), { recursive: true });
  await Promise.all(artifactPaths.map((artifact) => writeFile(artifact, `binary-${artifact}`)));
  const inventoryPath = path.join(workspaceRoot, ".tmp", "full-regression", `failure-inventory-${process.pid}.json`);
  const fingerprint = { algorithm: "test", algorithm_version: 1, files: [], digest: "b".repeat(64) };
  const artifacts = artifactPaths.map((artifact, index) => JSON.stringify({
    reason: "compiler-artifact",
    package_id: `pkg-${index} 0.1.0`,
    target: { name: `test-${index}`, kind: ["test"], test: true },
    profile: { test: true },
    executable: artifact,
  })).join("\n");
  await buildFullRegressionArtifacts({
    cwd: process.cwd(), inventoryPath,
    collectFingerprint: async () => fingerprint,
    run: async () => ({ stdout: artifacts, stderr: "" }),
  });
  let releaseSiblingStarted;
  const siblingStarted = new Promise((resolve) => { releaseSiblingStarted = resolve; });
  let siblingAborted = false;
  try {
    await assert.rejects(executeFullRegression({
      cpuCount: 8,
      cwd: process.cwd(),
      inventoryPath,
      baseEnv: {},
      collectFingerprint: async () => fingerprint,
      run: async (options) => {
        if (options.command === process.execPath) {
          await new Promise((resolve) => options.signal.addEventListener("abort", resolve, { once: true }));
          throw options.signal.reason;
        }
        if (options.command === artifactPaths[0]) {
          await siblingStarted;
          throw new Error("injected test failure");
        }
        if (options.command === artifactPaths[1]) {
          releaseSiblingStarted();
          await new Promise((_, reject) => options.signal.addEventListener("abort", () => {
            siblingAborted = true;
            reject(options.signal.reason);
          }, { once: true }));
        }
        throw new Error(`unexpected post-failure command: ${options.command}`);
      },
    }), (error) => {
      assert.match(error.message, /prebuilt Rust test binaries failed/i);
      assert.match(error.message, /pkg-0 0\.1\.0 test:test-0 failed: injected test failure/);
      return true;
    });
    assert.equal(siblingAborted, true);
  } finally {
    await Promise.all([
      ...artifactPaths.map((artifact) => rm(artifact, { force: true })),
      rm(inventoryPath, { force: true }),
    ]);
  }
});

it("rejects changed binary bytes before starting any test", async () => {
  const fixture = await createSealedFixture();
  let runCount = 0;
  try {
    await writeFile(fixture.artifact, "tampered test binn");
    await assert.rejects(executeFullRegression({
      cwd: process.cwd(), inventoryPath: fixture.inventoryPath,
      collectFingerprint: async () => fixture.fingerprint,
      run: async () => { runCount += 1; },
    }), /SHA-256 mismatch/i);
    assert.equal(runCount, 0);
  } finally {
    await fixture.cleanup();
  }
});

it("rejects source drift before starting any test", async () => {
  const fixture = await createSealedFixture();
  let runCount = 0;
  try {
    await assert.rejects(executeFullRegression({
      cwd: process.cwd(), inventoryPath: fixture.inventoryPath,
      collectFingerprint: async () => ({ ...fixture.fingerprint, digest: "c".repeat(64) }),
      run: async () => { runCount += 1; },
    }), /malformed, stale, or bound/i);
    assert.equal(runCount, 0);
  } finally {
    await fixture.cleanup();
  }
});

it("rejects source drift detected after the shared execution phase", async () => {
  const fixture = await createSealedFixture();
  let fingerprintCall = 0;
  try {
    await assert.rejects(executeFullRegression({
      cwd: process.cwd(), inventoryPath: fixture.inventoryPath,
      collectFingerprint: async () => fingerprintCall++ === 0
        ? fixture.fingerprint
        : { ...fixture.fingerprint, digest: "d".repeat(64) },
      run: async ({ args }) => ({ stdout: args.includes("--list") && args.includes("--exact")
        ? "lifecycle::year_boundary_keeps_company_operations_and_disclosure_state_authoritative: test\n"
        : "", stderr: "" }),
    }), /source changed during test execution/i);
  } finally {
    await fixture.cleanup();
  }
});

it("rejects inventory paths outside workspace .tmp", async () => {
  await assert.rejects(buildFullRegressionArtifacts({
    cwd: process.cwd(),
    inventoryPath: path.join(path.parse(process.cwd()).root, "tmp", `inventory-${process.pid}.json`),
    collectFingerprint: async () => ({ algorithm: "test", algorithm_version: 1, files: [], digest: "e".repeat(64) }),
    run: async () => { throw new Error("must reject before build"); },
  }), /strict child of workspace \.tmp/i);
});

it("rejects an artifact redirected outside Cargo target through an intermediate symlink", async () => {
  const workspaceRoot = await resolveWorkspaceRoot(process.cwd());
  const target = path.join(workspaceRoot, ".tmp", "build-cache", "full-regression");
  const linkDirectory = path.join(target, `symlink-chain-${process.pid}-${Date.now()}`);
  const artifact = path.join(linkDirectory, "test-binary");
  const escapedDirectory = path.join(workspaceRoot, ".tmp", `escaped-artifact-${process.pid}-${Date.now()}`);
  const inventoryPath = path.join(workspaceRoot, ".tmp", "full-regression", `symlink-inventory-${process.pid}-${Date.now()}.json`);
  const fingerprint = { algorithm: "test", algorithm_version: 1, files: [], digest: "f".repeat(64) };
  await mkdir(linkDirectory, { recursive: true });
  await writeFile(artifact, "sealed symlink binary");
  try {
    await buildFullRegressionArtifacts({
      cwd: process.cwd(), inventoryPath,
      collectFingerprint: async () => fingerprint,
      run: async () => ({ stdout: `${JSON.stringify({ reason: "compiler-artifact", package_id: "fixture", target: { name: "fixture", kind: ["test"], test: true }, profile: { test: true }, executable: artifact })}\n`, stderr: "" }),
    });
    await rm(linkDirectory, { recursive: true });
    await mkdir(escapedDirectory);
    await writeFile(path.join(escapedDirectory, "test-binary"), "sealed symlink binary");
    await symlink(escapedDirectory, linkDirectory, "dir");
    await assert.rejects(executeFullRegression({
      cwd: process.cwd(), inventoryPath,
      collectFingerprint: async () => fingerprint,
      run: async () => { throw new Error("must reject before execution"); },
    }), /escaped the Cargo target/i);
  } finally {
    await Promise.all([
      rm(linkDirectory, { recursive: true, force: true }),
      rm(escapedDirectory, { recursive: true, force: true }),
      rm(inventoryPath, { force: true }),
    ]);
  }
});

it("does not publish an inventory when source changes during the build", async () => {
  const workspaceRoot = await resolveWorkspaceRoot(process.cwd());
  const target = path.join(workspaceRoot, ".tmp", "build-cache", "full-regression");
  const artifact = path.join(target, "debug", "deps", `drift-fixture-${process.pid}`);
  const inventoryPath = path.join(workspaceRoot, ".tmp", "full-regression", `drift-inventory-${process.pid}.json`);
  await mkdir(path.dirname(artifact), { recursive: true });
  await writeFile(artifact, "binary");
  let fingerprintCall = 0;
  try {
    await assert.rejects(buildFullRegressionArtifacts({
      cwd: process.cwd(), inventoryPath,
      collectFingerprint: async () => ({ algorithm: "test", algorithm_version: 1, files: [], digest: `${++fingerprintCall}`.padStart(64, "0") }),
      run: async () => ({ stdout: `${JSON.stringify({ reason: "compiler-artifact", package_id: "fixture", target: { name: "fixture", kind: ["test"], test: true }, profile: { test: true }, executable: artifact })}\n`, stderr: "" }),
    }), /source changed during.*build/i);
    await assert.rejects(readFile(inventoryPath), /ENOENT/);
  } finally {
    await Promise.all([rm(artifact, { force: true }), rm(inventoryPath, { force: true })]);
  }
});

it("rejects a workspace .tmp symlink before creating cache directories", async () => {
  const workspaceRoot = await resolveWorkspaceRoot(process.cwd());
  const fixtureRoot = await mkdtemp(path.join(workspaceRoot, ".tmp", "workspace-path-test-"));
  const sourceRoot = path.join(fixtureRoot, "source");
  const escaped = path.join(fixtureRoot, "escaped");
  await Promise.all([mkdir(path.join(sourceRoot, ".git"), { recursive: true }), mkdir(escaped)]);
  await symlink(escaped, path.join(sourceRoot, ".tmp"), "dir");
  try {
    await assert.rejects(prepareWorkspacePaths({ sourceRoot, scope: "test" }), /symbolic link|symlink/i);
  } finally {
    await rm(fixtureRoot, { recursive: true, force: true });
  }
});

it("gives cold build and test execution independent five-minute hard deadlines", async () => {
  let nowMs = 1_000;
  const calls = [];
  const result = await runFullRegression({
    now: () => nowMs,
    run: async (options) => {
      calls.push(options);
      nowMs += 120_000;
    },
    cwd: process.cwd(),
  });
  assert.deepEqual(calls.map((call) => call.timeoutMs), [300_000, 300_000]);
  assert.deepEqual(calls.map((call) => call.args[1]), ["--internal-build", "--internal-execute"]);
  assert.equal(result.wall_ms, 240_000);
  assert.deepEqual(result.phases.map((phase) => phase.wall_ms), [120_000, 120_000]);
});

it("wraps an explicit phase around its complete internal workflow with one external five-minute deadline", async () => {
  const calls = [];
  await runFullRegressionPhase({
    mode: "build",
    cwd: process.cwd(),
    inventoryPath: "/workspace/.tmp/inventory.json",
    run: async (options) => { calls.push(options); },
  });
  assert.equal(calls.length, 1);
  assert.equal(calls[0].timeoutMs, 300_000);
  assert.equal(calls[0].env.STOCK_GAME_FULL_REGRESSION_INTERNAL_PHASE, "1");
  assert.deepEqual(calls[0].args.slice(1), ["--internal-build", "--inventory", "/workspace/.tmp/inventory.json"]);
});

it("canonical 长验过滤名必须与 Rust 测试源的真实用例一致，防止改名漂移", async () => {
  const required = regression.REQUIRED_LONG_VALIDATIONS;
  assert.ok(Array.isArray(required) && required.length > 0, "REQUIRED_LONG_VALIDATIONS 必须导出且非空");
  for (const validation of required) {
    assert.equal(typeof validation.targetLabel, "string");
    assert.equal(typeof validation.filter, "string");
    const [targetKind, targetName] = validation.targetLabel.split(":");
    assert.equal(targetKind, "test", "当前静态核对只覆盖集成测试目标");
    const [moduleName, ...caseParts] = validation.filter.split("::");
    const caseName = caseParts.join("::");
    const targetRoot = path.join(path.resolve(import.meta.dirname, ".."), "packages", "engine", "tests", targetName);
    const declared = await readFile(path.join(targetRoot, "main.rs"), "utf8").catch(() => {
      throw new Error(`长验目标目录不存在：${targetRoot}`);
    });
    assert.match(declared, new RegExp(`^mod ${moduleName};$`, "m"),
      `${validation.targetLabel} 的 main.rs 未声明 mod ${moduleName}`);
    const moduleSource = await readFile(path.join(targetRoot, `${moduleName}.rs`), "utf8");
    assert.match(moduleSource, new RegExp(`fn ${caseName}\\(`),
      `过滤名 ${validation.filter} 在 ${moduleName}.rs 中没有同名用例，长验会 0-case 假绿`);
  }
});

it("长验过滤命中 0 条用例时必须显式报错，不允许空集静默绿", async () => {
  const fixture = await createSealedFixture();
  let ordinaryExecutions = 0;
  try {
    await assert.rejects(executeFullRegression({
      cpuCount: 1,
      inventoryPath: fixture.inventoryPath,
      collectFingerprint: async () => fixture.fingerprint,
      runScripts: async () => ({ file_count: 0 }),
      run: async ({ args }) => {
        if (args.includes("--list") && args.includes("--exact")) return { stdout: "" };
        if (args.includes("--list")) return { stdout: args.includes("--ignored") ? "" : "ordinary: test\n" };
        ordinaryExecutions += 1;
      },
    }), /resolved to 0 test cases/);
    assert.equal(ordinaryExecutions, 1, "只有普通 case 被执行，长验空集必须在运行前拦截");
  } finally {
    await fixture.cleanup();
  }
});

it("validates explicit build and execute CLI modes", () => {
  assert.deepEqual(parseFullRegressionArgs(["build", "--inventory", "/workspace/.tmp/inventory.json"]), {
    mode: "build", inventoryPath: "/workspace/.tmp/inventory.json",
  });
  assert.deepEqual(parseFullRegressionArgs(["execute", "--inventory", "/workspace/.tmp/inventory.json"]), {
    mode: "execute", inventoryPath: "/workspace/.tmp/inventory.json",
  });
  assert.deepEqual(parseFullRegressionArgs([]), { mode: "all", inventoryPath: undefined });
  assert.deepEqual(parseFullRegressionArgs(["--internal-build"]), { mode: "build", inventoryPath: undefined, internal: true });
  assert.throws(() => parseFullRegressionArgs(["unknown"]), /usage:/i);
  assert.throws(() => parseFullRegressionArgs(["build"]), /--inventory/i);
});

it("显式 target-dir 在 all 和内部两阶段解析并完整转发", { timeout: 10000 }, async () => {
  const targetDir = path.resolve("external-target");
  const inventoryPath = path.resolve(".tmp/inventory.json");
  for (const mode of ["all", "build", "execute", "--internal-build", "--internal-execute"]) {
    for (const args of [["--inventory", inventoryPath, "--target-dir", targetDir], ["--target-dir", targetDir, "--inventory", inventoryPath]]) {
      const parsed = parseFullRegressionArgs([mode, ...args]);
      assert.equal(parsed.targetDir, targetDir);
      assert.equal(parsed.inventoryPath, inventoryPath);
    }
  }
  assert.equal(parseFullRegressionArgs(["all", "--target-dir", targetDir]).targetDir, targetDir);
  assert.equal(parseFullRegressionArgs(["--internal-build", "--target-dir", targetDir]).targetDir, targetDir);
  const calls = [];
  await runFullRegression({ targetDir, inventoryPath, run: async (options) => { calls.push(options); } });
  assert.deepEqual(calls.map(({ args }) => args.slice(1)), ["build", "execute"].map((mode) =>
    [`--internal-${mode}`, "--inventory", inventoryPath, "--target-dir", targetDir]));
  assert.ok(calls.every((call) => call.timeoutMs === 300000));
});

it("target-dir 与 inventory 参数拒绝重复、空值、缺值及未知选项", { timeout: 10000 }, () => {
  for (const args of [
    ["--target-dir", ""], ["--target-dir"], ["--target-dir", "--inventory", "file"],
    ["--target-dir", "a", "--target-dir", "b"], ["--inventory", "a", "--inventory", "b"],
    ["--inventory", ""], ["--unknown", "value"],
  ]) assert.throws(() => parseFullRegressionArgs(["all", ...args]), /target-dir|inventory|unknown/i);
});

it("外置 tiny artifact 在同一 target 封存和执行，跨 target inventory 显式拒绝", { timeout: 10000 }, async () => {
  const fixture = await createExternalTargetFixture();
  try {
    const { cwd, targetDir } = fixture;
    const artifact = path.join(targetDir, "fixture-binary");
    await writeFile(artifact, "tiny sealed artifact");
    const fingerprint = { algorithm: "test", algorithm_version: 1, files: [], digest: "b".repeat(64) };
    const options = { cwd, targetDir: `${targetDir}${path.sep}.`, cpuCount: 4,
      baseEnv: { CARGO_TARGET_DIR: "/hostile-target", TMPDIR: "/hostile-tmp" },
      collectFingerprint: async () => fingerprint };
    const checkEnv = (env) => {
      assert.equal(env.CARGO_TARGET_DIR, targetDir);
      for (const name of ["TMPDIR", "TMP", "TEMP"]) assert.equal(env[name], path.join(cwd, ".tmp/process-tmp/full-regression"));
    };
    const build = await buildFullRegressionArtifacts({ ...options, run: async ({ env }) => {
      checkEnv(env);
      return { stdout: JSON.stringify({ reason: "compiler-artifact", package_id: "fixture", target: { name: "company_scenarios", kind: ["test"], test: true }, profile: { test: true }, executable: artifact }) };
    } });
    assert.equal(build.inventory.cargo_target_dir, targetDir);
    assert.equal(build.inventory.artifacts[0].executable_relative_path, "../target/fixture-binary");
    const steps = fullRegressionSteps(4, cwd, options.baseEnv, cwd, targetDir);
    steps.forEach(({ env }) => checkEnv(env));
    let ordinaryExecutions = 0;
    let scriptsObserved = false;
    const execute = { ...options, inventoryPath: build.inventory_path,
      runScripts: async ({ env }) => { checkEnv(env); scriptsObserved = true; return { file_count: 1 }; },
      run: async ({ command, args, env }) => {
        checkEnv(env);
        if (args.includes("--list")) return { stdout: args.includes("--exact")
          ? `${regression.REQUIRED_LONG_VALIDATIONS[0].filter}: test\n`
          : (args.includes("--ignored") ? "" : "ordinary: test\n") };
        if (command === artifact && args[0] === "ordinary") ordinaryExecutions += 1;
        return { stdout: "" };
      } };
    await executeFullRegression(execute);
    assert.equal(ordinaryExecutions, 1);
    assert.equal(scriptsObserved, true);
    const wrongTarget = path.join(fixture.root, "wrong-target");
    await mkdir(wrongTarget);
    await assert.rejects(executeFullRegression({ ...execute, targetDir: wrongTarget,
      run: async () => { assert.fail("跨 target inventory 不能启动测试"); } }), /malformed, stale, or bound/i);
    await assert.rejects(executeFullRegression({ ...execute, targetDir: undefined,
      run: async () => { assert.fail("缺少 target-dir 不能复用外置 inventory"); } }), /malformed, stale, or bound/i);
    await writeFile(artifact, "tiny changed artifact");
    await assert.rejects(executeFullRegression(execute), /sealed inventory|SHA-256 mismatch/i);
  } finally { await fixture.cleanup(); }
});

it("外置 target 拒绝相对路径、缺失、文件及目标或祖先 symbolic link", { timeout: 10000 }, async () => {
  const fixture = await createExternalTargetFixture();
  try {
    const file = path.join(fixture.root, "file");
    const directLink = path.join(fixture.root, "target-link");
    const ancestorLink = path.join(fixture.root, "ancestor-link");
    await Promise.all([writeFile(file, "not a directory"), symlink(fixture.targetDir, directLink, "dir"), symlink(fixture.root, ancestorLink, "dir")]);
    const invalid = ["relative-target", "", path.join(fixture.root, "missing"), file, directLink, path.join(ancestorLink, "target"), `${directLink}${path.sep}..${path.sep}target`];
    const results = await Promise.allSettled(invalid.flatMap((targetDir) => [buildFullRegressionArtifacts, executeFullRegression].map(async (phase) => {
      await assert.rejects(phase({ cwd: fixture.cwd, targetDir,
        collectFingerprint: async () => { assert.fail("非法 target 必须在扫描源码前拒绝"); },
        run: async () => { assert.fail("非法 target 不能启动 child"); } }), /target.*absolute|target.*directory|symbolic link|target.*unavailable/i);
    })));
    for (const result of results) if (result.status === "rejected") throw result.reason;
  } finally { await fixture.cleanup(); }
});

it("外置 target 在 POSIX 普通用户下拒绝不可写目录", {
  timeout: 10000, skip: process.platform === "win32" || process.getuid?.() === 0,
}, async () => {
  const fixture = await createExternalTargetFixture();
  try {
    await chmod(fixture.targetDir, 0o555);
    for (const phase of [buildFullRegressionArtifacts, executeFullRegression]) {
      await assert.rejects(phase({ cwd: fixture.cwd, targetDir: fixture.targetDir,
        collectFingerprint: async () => { assert.fail("不可写 target 必须在扫描源码前拒绝"); },
      }), /target directory is not writable/i);
    }
  } finally {
    await chmod(fixture.targetDir, 0o755);
    await fixture.cleanup();
  }
});

it("rejects direct invocation of an unsupervised internal phase worker", async () => {
  await assert.rejects(main(["--internal-build"], {}), /external five-minute supervisor/i);
});

for (const firstError of ["filesystem", "SHA"]) it(`较早 ${firstError} 错误优先于后项非法 descriptor`, { timeout: 10000 }, async () => {
  const fixture = await createSealedFixture();
  try {
    const decoded = JSON.parse(await readFile(fixture.inventoryPath, "utf8"));
    decoded.artifacts.push({ label: "非法后项" });
    if (firstError === "filesystem") await rm(fixture.artifact);
    else decoded.artifacts[0].sha256 = "0".repeat(64);
    await writeFile(fixture.inventoryPath, JSON.stringify(resealInventory(decoded)));
    let runs = 0;
    await assert.rejects(executeFullRegression({
      cpuCount: 2, baseEnv: {}, inventoryPath: fixture.inventoryPath,
      collectFingerprint: async () => fixture.fingerprint,
      run: async () => { runs += 1; },
    }), firstError === "filesystem" ? /ENOENT/ : /SHA-256 mismatch/);
    assert.equal(runs, 0);
  } finally { await fixture.cleanup(); }
});

it("真实 hash worker 崩溃时 build 不发布、execute 不启动测试且全部线程退出", { timeout: 10000 }, async () => {
  const fixture = await createSealedFixture();
  const observed = [];
  const hashArtifacts = (entries, options) => hashArtifactFiles(entries, { ...options,
    createWorker: () => {
      const worker = new Worker(new URL("data:text/javascript,throw new Error('注入 hash worker 崩溃')"));
      observed.push(worker);
      return worker;
    },
  });
  let runs = 0;
  const unpublished = `${fixture.inventoryPath}.unpublished`;
  try {
    await assert.rejects(buildFullRegressionArtifacts({
      cwd: process.cwd(), cpuCount: 2, baseEnv: {}, inventoryPath: unpublished, hashArtifacts,
      collectFingerprint: async () => fixture.fingerprint,
      run: async () => ({ stdout: JSON.stringify({ reason: "compiler-artifact", package_id: "fixture",
        target: { name: "fixture", kind: ["test"], test: true }, profile: { test: true }, executable: fixture.artifact }) }),
    }), /注入 hash worker 崩溃/);
    await assert.rejects(readFile(unpublished), /ENOENT/);
    await assert.rejects(executeFullRegression({
      cwd: process.cwd(), cpuCount: 2, baseEnv: {}, inventoryPath: fixture.inventoryPath, hashArtifacts,
      collectFingerprint: async () => fixture.fingerprint,
      run: async () => { runs += 1; },
    }), /注入 hash worker 崩溃/);
    assert.equal(runs, 0);
    assert.equal(observed.length, 2);
    assert.ok(observed.every((worker) => worker.threadId === -1));
  } finally { await rm(unpublished, { force: true }); await fixture.cleanup(); }
});
