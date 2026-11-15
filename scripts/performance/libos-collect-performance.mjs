#!/usr/bin/env node
// 在进程外长验收监督器 + taskset 下运行。两侧交错采集，不同时争用同一组 CPU。
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { mkdir, readFile, realpath, writeFile } from "node:fs/promises";
import { arch, release } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { SCENARIO } from "../fixtures/libos-product-scenario.mjs";
import { runLibosProductExperiment } from "../libos-product-parity.mjs";
import { compareLibosPerformance } from "./libos-paired-performance.mjs";
import { measuredAuthority } from "./libos-performance-window.mjs";

const baselineRequired = "4961bd2740d4ffbed83528727cb571047c44bb3d";
const defaultWarmupTicks = 8;
const sha = (data) => createHash("sha256").update(data).digest("hex");

function exactJson(value) {
  if (Array.isArray(value)) return "[" + value.map(exactJson).join(",") + "]";
  if (value !== null && typeof value === "object") return "{" + Object.keys(value).sort()
    .map((key) => JSON.stringify(key) + ":" + exactJson(value[key])).join(",") + "}";
  return JSON.stringify(value);
}

function git(root, ...args) {
  return execFileSync("git", ["-C", root, ...args], { maxBuffer: 128 * 1024 * 1024 });
}

async function identity(root) {
  const sourceSha = git(root, "rev-parse", "HEAD").toString().trim();
  const untracked = git(root, "ls-files", "--others", "--exclude-standard", "-z").toString().split("\0").filter(Boolean);
  const records = await Promise.all(untracked.map(async (relative) => [relative, sha(await readFile(path.join(root, relative)))]));
  const diff = git(root, "diff", "--binary", "--no-ext-diff", "--no-renames", "HEAD");
  const locks = await Promise.all(["Cargo.lock", "pnpm-lock.yaml", "rust-toolchain.toml"].map(async (relative) =>
    [relative, sha(await readFile(path.join(root, relative)))]));
  return {
    sourceSha,
    sourceTreeSha256: sha(exactJson({ sourceSha, records, diff: sha(diff) })),
    lockSha256: sha(exactJson(locks)),
    clean: records.length === 0 && diff.length === 0,
  };
}

export function parseCpuSet(text) {
  const list = text.match(/^Cpus_allowed_list:\s*([0-9,-]+)$/mu)?.[1];
  assert.ok(list, "Linux 未提供可核验的 CPU affinity");
  const cpuSet = [];
  for (const token of list.split(",")) {
    const match = /^(\d+)(?:-(\d+))?$/u.exec(token);
    assert.ok(match, "CPU affinity 格式不合法");
    const start = Number(match[1]), end = Number(match[2] ?? match[1]);
    assert.ok(Number.isSafeInteger(start) && Number.isSafeInteger(end)
      && end >= start && end - start < 1024 && start > (cpuSet.at(-1) ?? -1), "CPU affinity 范围不合法");
    for (let cpu = start; cpu <= end; cpu++) cpuSet.push(cpu);
  }
  assert.ok(cpuSet.length >= 2 && cpuSet.length <= 1024, "需绑定至少两个逻辑 CPU");
  return cpuSet;
}

function checkThresholds(config) {
  const keys = ["throughputLossPct", "p50RisePct", "p95RisePct", "cpuPerTickRisePct",
    "memoryRisePct", "startupRisePct", "shutdownRisePct"];
  assert.deepEqual(Object.keys(config).sort(), keys.sort(), "采集前须明确全部性能门槛");
  for (const [key, value] of Object.entries(config)) {
    assert.ok(Number.isFinite(value) && value >= 0 && (key !== "throughputLossPct" || value <= 100),
      "非法性能门槛：" + key);
  }
}

function toolchainVersion(root, target) {
  const toolchain = (tool) => execFileSync("rustc", [...(tool === undefined ? [] : ["+" + tool]), "--version", "--verbose"], {
    cwd: root, encoding: "utf8",
  }).split("\n").filter((line) => /^(rustc |commit-hash: |host: )/u.test(line)).join(";");
  return target === "native" ? toolchain() : toolchain("nightly-2026-09-05");
}

async function writeJson(output, name, value) {
  await writeFile(path.join(output, name + ".json"), JSON.stringify(value, null, 2) + "\n", { flag: "wx" });
}

async function main(argv) {
  const [target, baselineRoot, candidateRoot, baselineArtifact, candidateArtifact, thresholdPath, output] = argv;
  assert.equal(process.platform, "linux", "当前采集器仅支持 Linux");
  assert.ok(["native", "browser"].includes(target) && argv.length === 7 &&
    [baselineRoot, candidateRoot, baselineArtifact, candidateArtifact, thresholdPath, output].every(path.isAbsolute),
  "用法：libos-collect-performance.mjs <native|browser> <baseline 源码根> <candidate 源码根> <baseline Server 路径|Chromium 路径> <candidate Server 路径|同一 Chromium 路径> <阈值 JSON> <新输出目录>（全部绝对路径）");
  const before = { root: await realpath(baselineRoot), executable: await realpath(baselineArtifact), name: "baseline" };
  const after = { root: await realpath(candidateRoot), executable: await realpath(candidateArtifact), name: "candidate" };
  assert.notEqual(before.root, after.root, "两侧必须是独立源码目录");
  assert.equal(target === "browser", before.executable === after.executable,
    "Native 必须分开保存两个制品；Browser 必须使用同一 Chromium");
  if (target === "native") {
    const digests = await Promise.all([before, after].map(async ({ executable }) => sha(await readFile(executable))));
    assert.notEqual(digests[0], digests[1], "不比较内容相同的两份 Native 制品");
    [before.digest, after.digest] = digests;
  }
  const identities = await Promise.all([identity(before.root), identity(after.root)]);
  assert.ok(identities[0].sourceSha === baselineRequired && identities[0].clean,
    "baseline 必须是未修改的历史 dev commit " + baselineRequired);
  assert.notEqual(identities[0].sourceTreeSha256, identities[1].sourceTreeSha256, "两侧源码不能相同");
  const thresholds = JSON.parse(await readFile(thresholdPath, "utf8"));
  checkThresholds(thresholds);
  const toolchain = toolchainVersion(before.root, target);
  assert.equal(toolchainVersion(after.root, target), toolchain, "两侧编译工具链不一致");
  const cpuSet = parseCpuSet(await readFile("/proc/self/status", "utf8"));
  const machineId = sha(await readFile("/proc/sys/kernel/random/boot_id"));
  const sources = await Promise.all(["../libos-product-parity.mjs", "./libos-performance-window.mjs",
    "./libos-collect-performance.mjs"].map((name) => readFile(new URL(name, import.meta.url))));
  const common = {
    target, platform: "linux-" + arch() + " " + release(), machineId,
    host: target === "native" ? "native-server-api-ws" : "browser-worker-shared-wasm",
    toolchain, executionVersion: sha(exactJson(sources.map(sha))),
    profile: "release", features: target === "native" ? ["host-parity"] : [],
    cpuSet, workerCount: 3, warmupTicks: defaultWarmupTicks,
    measuredTicks: SCENARIO.ticksPerDay - defaultWarmupTicks,
  };
  assert.ok(common.measuredTicks >= 20, "测量窗不足 20 个 tick");
  await mkdir(output);
  await writeJson(output, "thresholds", thresholds);
  console.log("LibOS 性能 " + target + "：CPU " + cpuSet.join(",") + "，1 Group / 2 Desk，3 对交错运行");
  const reports = new Map();
  for (let i = 0; i < 3; i++) {
    const pairId = "pair-" + (i + 1);
    for (const side of i % 2 === 0 ? [before, after] : [after, before]) {
      const run = await runLibosProductExperiment({
        mode: target, sourceRoot: side.root,
        ...(target === "native" ? { serverBin: side.executable } : { browserExecutable: side.executable }),
        warmupTicks: common.warmupTicks,
      });
      const { facts, receiptWitness, summary } = measuredAuthority(run.witness, { ...common, activeGroups: 1 });
      const sample = {
        pairId, measurementId: randomUUID(), ...run.witness.metrics,
        authority: { ...summary, factsSha256: sha(exactJson(facts)), readyReceiptSha256: sha(exactJson(receiptWitness)) },
      };
      const expectedNames = ["elapsedNs", "cpuNs", "memoryPeak", "startupNs", "shutdownNs"];
      assert.ok(expectedNames.every((name) => Number.isSafeInteger(sample[name]) && sample[name] > 0), "性能窗口观测缺失或分辨率不足");
      const provenance = identities[side === before ? 0 : 1];
      const context = { ...common, hostVersion: run.hostVersion, workloadSha256: run.fixtureSha256 };
      const artifactSha256 = target === "native" ? run.artifact.nativeSha256 :
        sha(exactJson({ wasm: run.artifact.wasmSha256, glue: run.artifact.glueSha256 }));
      if (target === "native") assert.equal(artifactSha256, side.digest, "Native 制品在测量中变化");
      const { sourceSha, sourceTreeSha256, lockSha256 } = provenance;
      const config = { schema: "libos-performance-receipt-v1", sourceSha, sourceTreeSha256, lockSha256, artifactSha256, context };
      if (!reports.has(side)) reports.set(side, { config, samples: [] });
      const report = reports.get(side);
      assert.deepEqual(report.config, config, "制品或采集上下文在配对测量中变化");
      report.samples.push(sample);
      await writeJson(output, side.name + "-" + pairId + "-evidence", {
        artifact: run.artifact, sample, facts, receiptWitness, parameters: { ...common, expectedGroups: 1 },
      });
      console.log("已完成 " + side.name + " " + pairId);
    }
  }
  assert.deepEqual(await Promise.all([identity(before.root), identity(after.root)]), identities, "测量中源码树变化");
  for (const side of [before, after]) {
    const { config, samples } = reports.get(side);
    await writeJson(output, side.name, { ...config, samples });
  }
  const baseline = reports.get(before), candidate = reports.get(after);
  const comparison = compareLibosPerformance({ ...baseline.config, samples: baseline.samples },
    { ...candidate.config, samples: candidate.samples }, thresholds);
  await writeJson(output, "comparison", comparison);
  process.exitCode = comparison.pass ? 0 : 1;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  try {
    await main(process.argv.slice(2));
  } catch (error) {
    console.error(error);
    process.exitCode = 2;
  }
}
