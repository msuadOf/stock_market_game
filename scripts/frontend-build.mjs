import { cp, lstat, mkdir, rm } from "node:fs/promises";
import { availableParallelism } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { executeBuildCommand, parseBuildArgs } from "./build-targets.mjs";
import { createFrontendCommands } from "./frontend-build-plan.mjs";
import { runBoundedCommand } from "./run-with-deadline.mjs";

const rootDirectory = path.resolve(import.meta.dirname, "..");

export function parseFrontendArgs(arguments_, cpuCount = availableParallelism()) {
  const remaining = [...arguments_];
  const pagesIndex = remaining.indexOf("--pages-base");
  let pagesBase;
  if (pagesIndex !== -1) {
    pagesBase = remaining[pagesIndex + 1];
    if (typeof pagesBase !== "string" || pagesBase !== pagesBase.trim() || pagesBase.split("/").some((part) => part === "." || part === "..") || !/^\/(?:[A-Za-z0-9_.-]+\/)*$/.test(pagesBase)) throw new Error("--pages-base requires / or a repository path such as /stock_market_game/.");
    remaining.splice(pagesIndex, 2);
    if (remaining.includes("--pages-base")) throw new Error("duplicate option: --pages-base");
  }
  const managerIndex = remaining.indexOf("--package-manager");
  let packageManager = "pnpm";
  if (managerIndex !== -1) {
    packageManager = remaining[managerIndex + 1];
    if (!["pnpm", "corepack"].includes(packageManager)) throw new Error("--package-manager requires pnpm or corepack.");
    remaining.splice(managerIndex, 2);
    if (remaining.includes("--package-manager")) throw new Error("duplicate option: --package-manager");
  }
  const options = parseBuildArgs(["webui", ...remaining], cpuCount);
  if (options.output !== undefined || options.frontendDist !== undefined) throw new Error("frontend build accepts only --jobs N, --dry-run, --package-manager pnpm|corepack or --help.");
  return { ...options, packageManager, pagesBase };
}

export function createFrontendPlan(options, { root = rootDirectory, host = process.platform === "win32" ? "windows" : process.platform === "darwin" ? "macos" : process.platform } = {}) {
  const frontend = path.join(root, "apps/web");
  const wasmPackage = path.join(root, "apps/web-wasm/wasm-pack-output/frontend/wasm/pkg");
  const env = { CARGO_TARGET_DIR: path.join(root, "target") };
  return { root, frontend, frontendDist: path.join(frontend, "dist"), wasmPackage, deadlineMs: 300000,
    commands: createFrontendCommands({ root, host, jobs: options.jobs, frontend, wasmPackage, env, pagesBase: options.pagesBase, packageManager: options.packageManager === undefined ? "pnpm" : options.packageManager }) };
}

async function ensureDirectory(root, directory) {
  const relative = path.relative(root, directory);
  if (!relative || relative.startsWith("..") || path.isAbsolute(relative)) throw new Error(`unsafe frontend directory: ${directory}`);
  let current = root;
  for (const component of relative.split(path.sep)) {
    current = path.join(current, component);
    let stat;
    try { stat = await lstat(current); } catch (error) {
      if (error.code !== "ENOENT") throw error;
      await mkdir(current);
      stat = await lstat(current);
    }
    if (stat.isSymbolicLink() || !stat.isDirectory()) throw new Error(`frontend directory is a symbolic link or non-directory: ${current}`);
  }
}

export async function executeFrontendBuild(plan, { commandRunner = executeBuildCommand } = {}) {
  await ensureDirectory(plan.root, path.dirname(plan.wasmPackage));
  await ensureDirectory(plan.root, path.join(plan.root, "target"));
  for (const command of plan.commands) {
    process.stdout.write(`[frontend build] ${command.command} ${command.args.join(" ")}\n`);
    if (command.command === "wasm-pack") {
      await ensureDirectory(plan.root, plan.wasmPackage);
      await rm(plan.wasmPackage, { recursive: true });
      await mkdir(plan.wasmPackage);
    }
    if (command.action === "copy-wasm") {
      const destination = path.join(plan.frontend, "wasm-pkg");
      await ensureDirectory(plan.root, plan.wasmPackage);
      await ensureDirectory(plan.root, destination);
      await rm(destination, { recursive: true });
      await cp(plan.wasmPackage, destination, { recursive: true, errorOnExist: true, force: false });
    } else await commandRunner(command);
  }
}

export async function main(arguments_, { root = rootDirectory, output = (message) => process.stdout.write(message), run = runBoundedCommand } = {}) {
  const options = parseFrontendArgs(arguments_);
  if (options.help) {
    output("Usage: node scripts/frontend-build.mjs [--jobs N] [--dry-run] [--package-manager pnpm|corepack] [--pages-base /REPOSITORY/]\nBuild: pinned pnpm, Rust nightly, wasm-pack and Node; output: apps/web/dist.\n");
    return;
  }
  const plan = createFrontendPlan(options, { root });
  output(`[frontend build] jobs=${options.jobs}; shared deadline=${plan.deadlineMs}ms; output=${plan.frontendDist}\n`);
  if (options.dryRun) {
    for (const command of plan.commands) output(`${command.command} ${command.args.join(" ")}\n`);
    return;
  }
  await run({ command: process.execPath, args: [fileURLToPath(import.meta.url), ...arguments_], cwd: root,
    env: { ...process.env, STOCK_FRONTEND_BUILD_WORKER: "1" }, timeoutMs: plan.deadlineMs, cleanupReserveMs: 1000 });
}

if (process.argv[1] !== undefined && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const arguments_ = process.argv.slice(2);
  const task = process.env.STOCK_FRONTEND_BUILD_WORKER === "1"
    ? executeFrontendBuild(createFrontendPlan(parseFrontendArgs(arguments_))) : main(arguments_);
  task.catch((error) => { process.stderr.write(`[frontend build] ${error.message}\n`); process.exitCode = 1; });
}
