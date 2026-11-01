import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, readdir, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";

const api = () => import("./frontend-build.mjs");

const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");
const requiredExports = ["restore_json", "facade_slot_init_for_restore_json", "restore_prepared", "clear_prepared_restore", "set_step_runtime_override", "init_wasm_wait", "wasm_wait_abort_layout", "shutdown_session"];

async function sourceFixture(root) {
  const inputs = ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo/config.toml", "apps/web-wasm/Cargo.toml", "apps/web-wasm/.cargo/config.toml", "apps/web-wasm/src/lib.rs", "packages/engine/Cargo.toml", "packages/engine/src/lib.rs"];
  const rows = [];
  for (const relative of inputs.sort()) {
    const bytes = relative === "apps/web-wasm/Cargo.toml" ? '[package]\nlicense-file = "../../LICENSE"\n' : "// fixture\n";
    await mkdir(path.dirname(path.join(root, relative)), { recursive: true });
    await writeFile(path.join(root, relative), bytes);
    rows.push({ path: relative, sha256: hash(bytes) });
  }
  return rows;
}

function packageFixture(diagnostic = false) {
  const names = diagnostic ? [...requiredExports, "private_diagnostic"] : requiredExports;
  const uleb = (value) => { const bytes = []; do { let byte = value & 0x7f; value >>>= 7; if (value !== 0) byte |= 0x80; bytes.push(byte); } while (value !== 0); return bytes; };
  const section = (id, payload) => [id, ...uleb(payload.length), ...payload];
  const type = section(1, [1, 0x60, 0, 0]);
  const functions = section(3, [names.length, ...names.map(() => 0)]);
  const exports = section(7, [names.length, ...names.flatMap((name, index) => [...uleb(Buffer.byteLength(name)), ...Buffer.from(name), 0, index])]);
  const code = section(10, [names.length, ...names.flatMap(() => [2, 0, 0x0b])]);
  return { wasm: Buffer.from([0, 0x61, 0x73, 0x6d, 1, 0, 0, 0, ...type, ...functions, ...exports, ...code]), glue: `// ${diagnostic ? "bad" : "fresh"} glue\nexport function ${names.join("() {}\nexport function ")}() {}\n` };
}

async function writePackageFixture(directory, bytes) {
  await writeFile(path.join(directory, "web_wasm_bg.wasm"), bytes.wasm);
  await writeFile(path.join(directory, "web_wasm.js"), bytes.glue);
}

test("shared frontend plan owns install, threaded WASM, copy, TypeScript, Vite and release checks on every host", async () => {
  const { createFrontendPlan } = await api();
  for (const host of ["linux", "windows", "macos"]) {
    const plan = createFrontendPlan({ jobs: 4 }, { host });
    assert.match(JSON.stringify(plan.commands), /--frozen-lockfile/);
    const wasm = plan.commands.find(({ command }) => command === "wasm-pack");
    // R4a 阶段 1（设计 §6.3 R3-1）：wasm32 链锁定 nightly-2026-09-05 + build-std
    // （flags 在 apps/web-wasm/.cargo/config.toml，按调用目录链生效）——
    // build 命令注入 RUSTUP_TOOLCHAIN；native 链（stable 1.96.1）不受影响。
    assert.ok(!wasm.args.includes("-Z"));
    assert.equal(wasm.env.RUSTUP_TOOLCHAIN, "nightly-2026-09-05");
    assert.equal(wasm.env.CARGO_BUILD_JOBS, "4");
    // 批 9 A7：RAYON 分量退役——断言缺省。
    assert.equal(wasm.env.RAYON_NUM_THREADS, undefined);
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
    env: { CARGO_BUILD_JOBS: "4", CARGO_TARGET_DIR: plan.cargoDirectory, STOCK_GAME_WASM_METADATA_MODULE: path.join(plan.work, "scripts/wasm-package-capability.mjs") } });
  assert.deepEqual(plan.commands.slice(1, 1 + commands.length), commands);
});

test("WASM copy replaces stale generated files only after source checks succeed", async () => {
  const { createFrontendPlan, executeFrontendBuild } = await api();
  const scratch = path.resolve(import.meta.dirname, "../.tmp");
  await mkdir(scratch, { recursive: true });
  const root = await mkdtemp(path.join(scratch, "frontend-build-"));
  try {
    const sourceRows = await sourceFixture(root);
    await writeFile(path.join(root, "LICENSE"), "fixture license");
    const plan = createFrontendPlan({ jobs: 4 }, { root, host: "linux" });
    await mkdir(plan.wasmPackage, { recursive: true });
    await mkdir(path.join(plan.frontend, "wasm-pkg"), { recursive: true });
    await writeFile(path.join(plan.wasmPackage, "stale-source"), "old generated file");
    await writeFile(path.join(plan.frontend, "wasm-pkg/stale"), "old");
    const bytes = packageFixture();
    const calls = [];
    await executeFrontendBuild(plan, { commandRunner: async (command) => {
      calls.push(command);
      if (command.command === "wasm-pack") await writePackageFixture(plan.wasmPackage, bytes);
    } });
    assert.deepEqual(await readdir(path.join(plan.frontend, "wasm-pkg")), ["LICENSE", "verified-restore-capability.json", "wasm-build-provenance.json", "web_wasm.js", "web_wasm_bg.wasm"]);
    assert.equal(await readFile(path.join(plan.frontend, "wasm-pkg/LICENSE"), "utf8"), "fixture license");
    assert.equal(await readFile(path.join(plan.frontend, "wasm-pkg/web_wasm.js"), "utf8"), bytes.glue);
    assert.deepEqual(await readFile(path.join(plan.frontend, "wasm-pkg/web_wasm_bg.wasm")), bytes.wasm);
    const provenance = { kind: "wasm-build-provenance", sourceSha256: hash(`${JSON.stringify(sourceRows, null, 2)}\n`), wasmSha256: hash(bytes.wasm), glueSha256: hash(bytes.glue), profile: "release", toolchain: "nightly-2026-09-05" };
    const provenanceBytes = Buffer.from(`${JSON.stringify(provenance, null, 2)}\n`);
    const capability = { kind: "verified-package", wasmSha256: provenance.wasmSha256, glueSha256: provenance.glueSha256, sourceSha256: provenance.sourceSha256, buildManifestSha256: hash(provenanceBytes), exports: [...requiredExports].sort() };
    assert.deepEqual(await readFile(path.join(plan.frontend, "wasm-pkg/wasm-build-provenance.json")), provenanceBytes);
    assert.deepEqual(await readFile(path.join(plan.frontend, "wasm-pkg/verified-restore-capability.json")), Buffer.from(`${JSON.stringify(capability, null, 2)}\n`));
    assert.match(calls.at(-1).args[0], /check-web-release-wasm/);
    await assert.rejects(executeFrontendBuild(plan, { commandRunner: async () => { throw new Error("compiler failed"); } }), /compiler failed/);
    assert.equal(await readFile(path.join(plan.frontend, "wasm-pkg/web_wasm.js"), "utf8"), bytes.glue);
    const failedCalls = [];
    await assert.rejects(executeFrontendBuild(plan, { commandRunner: async (command) => {
      failedCalls.push(command);
      if (command.command === "wasm-pack") await writePackageFixture(plan.wasmPackage, packageFixture(true));
      if (command.args[0].endsWith("check-web-release-wasm.mjs")) throw new Error("private diagnostic export");
    } }), /private diagnostic export/);
    assert.equal(await readFile(path.join(plan.frontend, "wasm-pkg/web_wasm.js"), "utf8"), bytes.glue);
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

test("Pages frontend explicitly uses repository base and production pages mode, without changing ordinary builds", async () => {
  const { parseFrontendArgs, createFrontendPlan } = await api();
  const options = parseFrontendArgs(["--pages-base", "/stock_market_game/", "--jobs", "4"]);
  const plan = createFrontendPlan(options, { host: "linux" });
  const vite = plan.commands.find(({ args }) => args[0].endsWith("vite.js"));
  assert.ok(vite.args.includes("pages"));
  assert.ok(vite.args.includes("/stock_market_game/"));
  const ordinary = createFrontendPlan({ jobs: 4 }, { host: "linux" }).commands.find(({ args }) => args[0].endsWith("vite.js"));
  assert.ok(!ordinary.args.includes("pages"));
  assert.equal(parseFrontendArgs(["--pages-base", "/stock.game/"]).pagesBase, "/stock.game/");
  for (const base of ["//evil/", "https://evil/", "/../", "/game", "/game/?x=1"]) assert.throws(() => parseFrontendArgs(["--pages-base", base]));
});

test("WASM copy refuses a symlink destination without changing the referenced files", async () => {
  const { createFrontendPlan, executeFrontendBuild } = await api();
  const scratch = path.resolve(import.meta.dirname, "../.tmp");
  await mkdir(scratch, { recursive: true });
  const root = await mkdtemp(path.join(scratch, "frontend-link-"));
  try {
    await sourceFixture(root);
    await writeFile(path.join(root, "LICENSE"), "fixture license");
    const plan = createFrontendPlan({ jobs: 4 }, { root, host: "linux" });
    const unrelated = path.join(root, "unrelated");
    await mkdir(plan.wasmPackage, { recursive: true });
    await mkdir(plan.frontend, { recursive: true });
    await mkdir(unrelated);
    await writeFile(path.join(unrelated, "keep"), "user data");
    await symlink(unrelated, path.join(plan.frontend, "wasm-pkg"), process.platform === "win32" ? "junction" : "dir");
    await assert.rejects(executeFrontendBuild(plan, { commandRunner: async (command) => {
      if (command.command === "wasm-pack") await writePackageFixture(plan.wasmPackage, packageFixture());
    } }), /symbolic link/);
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


test("frontend 构建在 LICENSE 变更后阻断后续复制", { timeout: 10000 }, async () => {
  const { createFrontendPlan, executeFrontendBuild } = await api();
  const scratch = path.resolve(import.meta.dirname, "../.tmp");
  await mkdir(scratch, { recursive: true });
  const root = await mkdtemp(path.join(scratch, "w0l-frontend-license-"));
  try {
    await mkdir(path.join(root, "apps/web-wasm/src"), { recursive: true });
    await mkdir(path.join(root, "packages/engine/src"), { recursive: true });
    await mkdir(path.join(root, ".cargo"), { recursive: true });
    await mkdir(path.join(root, "apps/web-wasm/.cargo"), { recursive: true });
    await writeFile(path.join(root, "apps/web-wasm/Cargo.toml"), '[package]\nlicense-file = "../../LICENSE"\n');
    await writeFile(path.join(root, "packages/engine/Cargo.toml"), "[package]\n");
    await writeFile(path.join(root, "Cargo.toml"), "[workspace]\n");
    await writeFile(path.join(root, "Cargo.lock"), "version = 4\n");
    await writeFile(path.join(root, "rust-toolchain.toml"), "[toolchain]\nchannel = \"nightly-2026-09-05\"\n");
    await writeFile(path.join(root, ".cargo/config.toml"), "[build]\n");
    await writeFile(path.join(root, "apps/web-wasm/.cargo/config.toml"), "[build]\n");
    await writeFile(path.join(root, "apps/web-wasm/src/lib.rs"), "// fixture\n");
    await writeFile(path.join(root, "packages/engine/src/lib.rs"), "// fixture\n");
    await writeFile(path.join(root, "LICENSE"), "fixture license");
    const plan = createFrontendPlan({ jobs: 4 }, { root, host: "linux" });
    await assert.rejects(executeFrontendBuild(plan, { commandRunner: async (command) => {
      if (command.command === "wasm-pack") await writeFile(path.join(root, "LICENSE"), "changed");
      else assert.ok(!command.args[0].endsWith("check-web-release-wasm.mjs"), "LICENSE failure must stop release checks");
    } }), /LICENSE.*changed/);
  } finally { await rm(root, { recursive: true, force: true }); }
});
