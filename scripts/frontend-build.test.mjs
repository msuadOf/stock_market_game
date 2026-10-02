import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, readdir, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";

const api = () => import("./frontend-build.mjs");

test("shared frontend plan owns install, threaded WASM, copy, TypeScript, Vite and release checks on every host", async () => {
  const { createFrontendPlan } = await api();
  for (const host of ["linux", "windows", "macos"]) {
    const plan = createFrontendPlan({ jobs: 4 }, { host });
    assert.match(JSON.stringify(plan.commands), /--frozen-lockfile/);
    const wasm = plan.commands.find(({ command }) => command === "wasm-pack");
    assert.ok(wasm.args.includes("build-std=panic_abort,std"));
    assert.equal(wasm.env.RUSTUP_TOOLCHAIN, "nightly-2026-09-05");
    assert.equal(wasm.env.CARGO_BUILD_JOBS, "4");
    assert.equal(wasm.env.RAYON_NUM_THREADS, "4");
    assert.ok(plan.commands.find(({ action }) => action === "copy-wasm"));
    assert.match(JSON.stringify(plan.commands), /typescript.*tsc/);
    assert.match(JSON.stringify(plan.commands), /vite.*build/);
    assert.match(plan.commands.at(-1).args[0], /check-web-release-wasm/);
    assert.equal(plan.commands.at(-1).args[1], plan.frontendDist);
    assert.equal(plan.deadlineMs, 300000);
  }
});

test("native UI builds use the same frontend command factory with isolated staging", async () => {
  const { createFrontendCommands } = await import("./frontend-build-plan.mjs");
  const { createBuildPlan, parseBuildArgs } = await import("./build-targets.mjs");
  const plan = createBuildPlan(parseBuildArgs(["webui", "--jobs", "4"]), { host: "linux", buildId: "fixture" });
  const commands = createFrontendCommands({ root: plan.root, host: plan.host, jobs: plan.jobs,
    frontend: plan.frontend, wasmPackage: plan.wasmPackage, staged: true,
    env: { CARGO_BUILD_JOBS: "4", CARGO_TARGET_DIR: plan.cargoDirectory } });
  assert.deepEqual(plan.commands.slice(1, 1 + commands.length), commands);
});

test("WASM copy replaces stale generated files only after source checks succeed", async () => {
  const { createFrontendPlan, executeFrontendBuild } = await api();
  const root = await mkdtemp(path.join(tmpdir(), "frontend-build-"));
  try {
    const plan = createFrontendPlan({ jobs: 4 }, { root, host: "linux" });
    await mkdir(plan.wasmPackage, { recursive: true });
    await mkdir(path.join(plan.frontend, "wasm-pkg"), { recursive: true });
    await writeFile(path.join(plan.wasmPackage, "stale-source"), "old generated file");
    await writeFile(path.join(plan.frontend, "wasm-pkg/stale"), "old");
    const calls = [];
    await executeFrontendBuild(plan, { commandRunner: async (command) => {
      calls.push(command);
      if (command.command === "wasm-pack") await writeFile(path.join(plan.wasmPackage, "web_wasm.js"), "fresh glue");
    } });
    assert.deepEqual(await readdir(path.join(plan.frontend, "wasm-pkg")), ["web_wasm.js"]);
    assert.equal(await readFile(path.join(plan.frontend, "wasm-pkg/web_wasm.js"), "utf8"), "fresh glue");
    assert.match(calls.at(-1).args[0], /check-web-release-wasm/);
    await assert.rejects(executeFrontendBuild(plan, { commandRunner: async () => { throw new Error("compiler failed"); } }), /compiler failed/);
    assert.equal(await readFile(path.join(plan.frontend, "wasm-pkg/web_wasm.js"), "utf8"), "fresh glue");
    const failedCalls = [];
    await assert.rejects(executeFrontendBuild(plan, { commandRunner: async (command) => {
      failedCalls.push(command);
      if (command.command === "wasm-pack") await writeFile(path.join(plan.wasmPackage, "web_wasm.js"), "bad glue");
      if (command.args[0].endsWith("check-web-release-wasm.mjs")) throw new Error("private diagnostic export");
    } }), /private diagnostic export/);
    assert.equal(await readFile(path.join(plan.frontend, "wasm-pkg/web_wasm.js"), "utf8"), "fresh glue");
    assert.ok(failedCalls.every(({ args }) => !args[0].endsWith("vite.js")));
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("frontend entry supervises the whole build once, and dry-run creates nothing", async () => {
  const { main } = await api();
  const root = await mkdtemp(path.join(tmpdir(), "frontend-supervisor-"));
  try {
    const calls = [];
    await main(["--jobs", "4", "--dry-run"], { root, output: () => {}, run: async (command) => { calls.push(command); } });
    assert.deepEqual(calls, []);
    assert.deepEqual(await readdir(root), []);
    await main(["--jobs", "4"], { root, output: () => {}, run: async (command) => { calls.push(command); } });
    assert.equal(calls.length, 1);
    assert.equal(calls[0].timeoutMs, 300000);
    assert.equal(calls[0].cleanupReserveMs, 1000);
    assert.equal(calls[0].env.STOCK_FRONTEND_BUILD_WORKER, "1");
    assert.match(calls[0].args[0], /frontend-build\.mjs$/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("frontend parser rejects duplicate, unsafe and native-only options", async () => {
  const { parseFrontendArgs } = await api();
  assert.equal(parseFrontendArgs([], 4).jobs, 4);
  assert.equal(parseFrontendArgs(["--package-manager", "corepack"], 4).packageManager, "corepack");
  for (const args of [["--jobs", "0"], ["--jobs"], ["--jobs", "4", "--jobs", "2"], ["--output", "elsewhere"], ["--compile-only"], ["--help", "garbage"], ["--package-manager"], ["--package-manager", "npm"], ["--package-manager", "pnpm", "--package-manager", "corepack"]]) {
    assert.throws(() => parseFrontendArgs(args, 4));
  }
});

test("WASM copy refuses a symlink destination without changing the referenced files", async () => {
  const { createFrontendPlan, executeFrontendBuild } = await api();
  const root = await mkdtemp(path.join(tmpdir(), "frontend-link-"));
  try {
    const plan = createFrontendPlan({ jobs: 4 }, { root, host: "linux" });
    const unrelated = path.join(root, "unrelated");
    await mkdir(plan.wasmPackage, { recursive: true });
    await mkdir(plan.frontend, { recursive: true });
    await mkdir(unrelated);
    await writeFile(path.join(unrelated, "keep"), "user data");
    await symlink(unrelated, path.join(plan.frontend, "wasm-pkg"), process.platform === "win32" ? "junction" : "dir");
    await assert.rejects(executeFrontendBuild(plan, { commandRunner: async () => {} }), /symbolic link/);
    assert.equal(await readFile(path.join(unrelated, "keep"), "utf8"), "user data");
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("legacy shell and batch frontend entries delegate to the shared build instead of duplicating it", async () => {
  for (const extension of ["sh", "bat"]) {
    const script = await readFile(new URL(`./wasm-build.${extension}`, import.meta.url), "utf8");
    assert.match(script, /frontend-build\.mjs.*--package-manager corepack/);
    assert.doesNotMatch(script, /wasm-pack build|pnpm.*--filter web build|xcopy|cp -r/);
  }
});
