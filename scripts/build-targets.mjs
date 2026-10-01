import { spawn } from "node:child_process";
import { cp, copyFile, lstat, mkdir, mkdtemp, readdir, readlink, realpath, rename, rm, symlink } from "node:fs/promises";
import { availableParallelism } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { randomUUID } from "node:crypto";
import { planCell } from "./desktop/build-matrix.mjs";
import { runBoundedCommand } from "./run-with-deadline.mjs";

const rootDirectory = path.resolve(import.meta.dirname, "..");
const targets = new Set(["desktop", "webui", "webui-server", "server"]);
const deadlineMs = 300000;

export function parseBuildArgs(arguments_, cpuCount = availableParallelism()) {
  if (arguments_.length === 0 || (arguments_.length === 1 && ["--help", "-h"].includes(arguments_[0]))) return { help: true };
  const target = arguments_[0];
  if (!targets.has(target)) throw new Error(`unknown target: ${target}; choose desktop|webui|webui-server|server.`);
  if (arguments_.length === 2 && ["--help", "-h"].includes(arguments_[1])) return { help: true };
  const options = { target, jobs: cpuCount, dryRun: false, help: false };
  const seen = new Set();
  for (let index = 1; index < arguments_.length; index += 1) {
    const argument = arguments_[index];
    if (seen.has(argument)) throw new Error(`duplicate option: ${argument}`);
    seen.add(argument);
    if (argument === "--dry-run") options.dryRun = true;
    else if (["--jobs", "--output"].includes(argument)) {
      const value = arguments_[++index];
      if (value === undefined || value.startsWith("--")) throw new Error(`${argument} requires a value.`);
      if (argument === "--jobs") {
        if (!/^[1-9][0-9]{0,5}$/.test(value)) throw new Error("--jobs must be a positive integer (maximum 999999).");
        options.jobs = Number(value);
      } else options.output = value;
    } else throw new Error(`unknown argument: ${argument}`);
  }
  if (!Number.isSafeInteger(options.jobs) || options.jobs < 1) throw new Error("cannot detect CPU jobs; specify --jobs N.");
  return options;
}

function hostName() {
  if (process.platform === "win32") return "windows";
  if (process.platform === "darwin") return "macos";
  if (process.platform === "linux") return "linux";
  throw new Error(`unsupported build host: ${process.platform}`);
}

function outputPath(root, target, output) {
  if (!targets.has(target)) throw new Error(`invalid artifact target: ${target}`);
  const base = path.join(root, "target/build-artifacts");
  const directory = output === undefined ? path.join(base, target) : path.resolve(root, output);
  if (path.dirname(directory) !== base || !/^[a-zA-Z0-9][a-zA-Z0-9_-]*$/.test(path.basename(directory))) {
    throw new Error("--output must be a direct child: target/build-artifacts/NAME.");
  }
  return directory;
}

export function createBuildPlan(options, { root = rootDirectory, host = hostName(), buildId = randomUUID(), nativeTarget = "<rustc-host>" } = {}) {
  if (!targets.has(options.target)) throw new Error(`invalid target: ${options.target}`);
  if (!["linux", "macos", "windows"].includes(host)) throw new Error(`unsupported host: ${host}`);
  if (!/^[a-zA-Z0-9_-]+$/.test(buildId)) throw new Error(`invalid build ID: ${buildId}`);
  const target = options.target;
  const work = path.join(root, "target/build-work", `${target}-${buildId}`);
  const cargoDirectory = target === "desktop" ? path.join(work, "cargo") : path.join(root, "target/build-cache", target);
  const frontend = path.join(work, "web");
  const wasmPackage = path.join(root, "apps/web-wasm/wasm-pack-output", buildId, "wasm/pkg");
  const env = { CARGO_BUILD_JOBS: String(options.jobs), CARGO_TARGET_DIR: cargoDirectory };
  const commands = [{ action: "native-target", command: "rustc", args: ["-vV"], cwd: root, env }];
  const nodeCommand = (args, cwd = root, extra = {}) => ({ command: process.execPath, args, cwd, env: { ...env, ...extra } });
  if (target !== "server") {
    commands.push(host === "windows"
      ? { command: "cmd.exe", args: ["/d", "/s", "/c", "corepack pnpm install --frozen-lockfile"], cwd: root, env }
      : { command: "corepack", args: ["pnpm", "install", "--frozen-lockfile"], cwd: root, env });
    commands.push({
      command: "wasm-pack", args: ["build", "apps/web-wasm", "--target", "web", "--release", "--out-dir", wasmPackage], cwd: root,
      env: { ...env, CARGO_TARGET_DIR: path.join(cargoDirectory, "wasm"), RUSTUP_TOOLCHAIN: "nightly-2026-09-05" },
    });
    commands.push(nodeCommand([path.join(root, "scripts/check-wasm-threading.mjs"), path.join(wasmPackage, "web_wasm.js")]));
    commands.push(nodeCommand([path.join(root, "scripts/check-web-release-wasm.mjs"), wasmPackage]));
    commands.push({ action: "stage-frontend", command: "stage-frontend", args: [frontend], cwd: root, env });
    commands.push(nodeCommand([path.join(root, "apps/web/node_modules/typescript/bin/tsc"), "--build", "--force"], frontend));
    commands.push(nodeCommand([path.join(root, "apps/web/node_modules/vite/bin/vite.js"), "build", "--outDir", path.join(frontend, "dist")], frontend, { RAYON_NUM_THREADS: String(options.jobs) }));
    commands.push(nodeCommand([path.join(root, "scripts/check-web-release-wasm.mjs"), path.join(frontend, "dist")]));
  }
  let files;
  let startArgs;
  if (target === "desktop") {
    const native = planCell(host, host);
    commands.push({
      command: native.build.command,
      args: [...native.build.arguments, "--target", nativeTarget, "--config", JSON.stringify({ build: { beforeBuildCommand: "", frontendDist: path.join(frontend, "dist") } }), "--", "--jobs", String(options.jobs), "--locked"],
      cwd: path.join(root, "apps/desktop/src-tauri"), env,
    });
    files = ["bundle/", "LICENSE"]; startArgs = [];
  } else {
    commands.push({ command: "cargo", args: ["build", "--locked", "--release", "-p", "server", "--bin", "server", "--jobs", String(options.jobs), "--target", nativeTarget, ...(target === "server" ? [] : ["--features", "web-ui"])], cwd: root, env });
    files = [host === "windows" ? "server.exe" : "server", ...(target === "server" ? [] : ["webui/"]), "LICENSE"];
    startArgs = ["--services", target === "webui-server" ? "all" : target];
  }
  return { target, host, buildId, jobs: options.jobs, deadlineMs, root, work, frontend, wasmPackage, wasmRoot: path.resolve(wasmPackage, "../.."), cargoDirectory, nativeTarget, commands, artifact: { directory: outputPath(root, target, options.output), files, startArgs },
    requirements: target === "server" ? "Build: Rust/Cargo only (use shell/batch for no Node). Deployment: native executable + LICENSE; no Node/Rust toolchain."
      : `Build: Node >=24.18.0, Corepack pnpm@11.19.0, Rust, nightly-2026-09-05 + rust-src/wasm32, wasm-pack.${target === "desktop" ? ` ${planCell(host, host).prerequisites} ${planCell(host, host).restriction} Desktop uses fresh Cargo output to prevent stale bundles; engine compilation cache is not reused. Dry-run does not validate native bundles or large-tree cleanup timing.` : " Deployment: native executable + webui/ + LICENSE; no Node/Rust toolchain; not Vite preview."}` };
}

async function statIfExists(filename) {
  try { return await lstat(filename); } catch (error) {
    if (error.code === "ENOENT") return undefined;
    throw error;
  }
}

async function safeDirectories(root, directory) {
  const relative = path.relative(root, directory);
  if (!relative || relative.startsWith("..") || path.isAbsolute(relative)) throw new Error(`unsafe build directory: ${directory}`);
  let current = root;
  for (const component of relative.split(path.sep)) {
    current = path.join(current, component);
    let stat = await statIfExists(current);
    if (stat === undefined) {
      await mkdir(current);
      stat = await lstat(current);
    }
    if (stat.isSymbolicLink() || !stat.isDirectory()) throw new Error(`build directory is a symbolic link or non-directory: ${current}`);
  }
}

async function assertNewOutput(root, directory) {
  await safeDirectories(root, path.dirname(directory));
  if (await statIfExists(directory) !== undefined) throw new Error(`refusing existing output: ${directory}; choose --output target/build-artifacts/NAME for a fresh repeat build.`);
}

async function assertTree(source, directoryExpected, bundleRoot) {
  const stat = await lstat(source);
  if (stat.isSymbolicLink()) {
    if (bundleRoot === undefined) throw new Error(`artifact source must not be a symlink: ${source}`);
    const relative = path.relative(bundleRoot, await realpath(source));
    if (!relative || relative.startsWith("..") || path.isAbsolute(relative)) throw new Error(`bundle symlink escaped its root: ${source}`);
    if (path.isAbsolute(await readlink(source))) throw new Error(`absolute bundle symlink is not relocatable: ${source}`);
    return;
  }
  if (directoryExpected) {
    if (!stat.isDirectory()) throw new Error(`artifact directory missing: ${source}`);
    const entries = await readdir(source, { withFileTypes: true });
    if (entries.length === 0 && (bundleRoot === undefined || source === bundleRoot)) throw new Error(`artifact directory is empty: ${source}`);
    await Promise.all(entries.map((entry) => assertTree(path.join(source, entry.name), entry.isDirectory(), bundleRoot)));
  } else if (!stat.isFile() || (stat.size === 0 && bundleRoot === undefined)) throw new Error(`artifact file missing or empty: ${source}`);
}

export async function publishArtifact({ root, target, executable, webRoot, bundleRoot, output, host = hostName(), stageDirectory }) {
  const directory = outputPath(root, target, output);
  await assertNewOutput(root, directory);
  const license = path.join(root, "LICENSE");
  await assertTree(license, false);
  if (target === "desktop") await assertTree(bundleRoot, true, bundleRoot);
  else {
    await assertTree(executable, false);
    if (target !== "server") {
      await assertTree(webRoot, true);
      await assertTree(path.join(webRoot, "index.html"), false);
    }
  }
  const stage = stageDirectory === undefined ? await mkdtemp(path.join(path.dirname(directory), `stage-${target}-`)) : stageDirectory;
  if (stageDirectory !== undefined) await mkdir(stage);
  try {
    await copyFile(license, path.join(stage, "LICENSE"));
    if (target === "desktop") await cp(bundleRoot, path.join(stage, "bundle"), { recursive: true, errorOnExist: true, force: false, verbatimSymlinks: true });
    else {
      await copyFile(executable, path.join(stage, host === "windows" ? "server.exe" : "server"));
      if (target !== "server") await cp(webRoot, path.join(stage, "webui"), { recursive: true, errorOnExist: true, force: false });
    }
    await assertNewOutput(root, directory);
    await rename(stage, directory);
    return directory;
  } finally {
    await rm(stage, { recursive: true, force: true });
  }
}

async function prepareOwnedDirectories(plan) {
  const owned = [];
  try {
    await assertNewOutput(plan.root, plan.artifact.directory);
    for (const directory of [plan.work, ...(plan.target === "server" ? [] : [plan.wasmRoot])]) {
      await safeDirectories(plan.root, path.dirname(directory));
      await mkdir(directory);
      owned.push(directory);
    }
    return owned;
  } catch (error) {
    await cleanupOwnedDirectories(owned);
    throw error;
  }
}

async function cleanupOwnedDirectories(owned) {
  await Promise.all(owned.map((directory) => rm(directory, { recursive: true, force: true })));
}

export async function stageFrontend(plan) {
  const source = path.join(plan.root, "apps/web");
  await mkdir(plan.frontend);
  const entries = await readdir(source, { withFileTypes: true });
  await Promise.all(entries.filter(({ name }) => !["node_modules", "dist", "build", "wasm-pkg", "wasm-diagnostics-pkg"].includes(name) && !name.startsWith(".env")).map(({ name }) =>
    cp(path.join(source, name), path.join(plan.frontend, name), { recursive: true, dereference: true })));
  const modules = path.join(plan.frontend, "node_modules");
  await mkdir(modules);
  const dependencies = await readdir(path.join(source, "node_modules"), { withFileTypes: true });
  await Promise.all(dependencies.filter(({ name }) => !name.startsWith(".")).map(async ({ name }) => {
    const actual = await realpath(path.join(source, "node_modules", name));
    await symlink(actual, path.join(modules, name), process.platform === "win32" ? "junction" : "dir");
  }));
  await cp(plan.wasmPackage, path.join(plan.frontend, "wasm-pkg"), { recursive: true, errorOnExist: true, force: false });
  await copyFile(path.join(plan.root, "LICENSE"), path.join(plan.work, "LICENSE"));
}

function compilerEnvironment(overrides) {
  const environment = { ...process.env, ...overrides };
  for (const name of ["VITE_ENGINE_HOST", "VITE_REMOTE_BASE_URL", "VITE_REMOTE_TOKEN"]) delete environment[name];
  return environment;
}

export function executeBuildCommand(command) {
  return new Promise((resolve, reject) => {
    const capture = command.action === "native-target";
    const child = spawn(command.command, command.args, { cwd: command.cwd, env: compilerEnvironment(command.env), stdio: capture ? ["ignore", "pipe", "inherit"] : "inherit", windowsHide: true, detached: false });
    let stdout = "";
    if (capture) child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.once("error", (error) => reject(new Error(`cannot start ${command.command}: ${error.message}; repair the build prerequisites.`)));
    child.once("close", (code, signal) => code === 0 ? resolve(stdout) : reject(new Error(`${command.command} failed: ${code === null ? signal : `exit ${code}`}; no artifact published.`)));
  });
}

export async function executeBuild(options, { commandRunner = executeBuildCommand, prepared = false, ...context } = {}) {
  let plan = createBuildPlan(options, context);
  const owned = prepared ? [] : await prepareOwnedDirectories(plan);
  try {
    const probe = await commandRunner(plan.commands[0]);
    const host = /^host: ([a-zA-Z0-9_-]+)$/m.exec(probe);
    if (host === null) throw new Error("rustc -vV did not report a valid native host; no artifact published.");
    plan = createBuildPlan(options, { ...context, buildId: plan.buildId, nativeTarget: host[1] });
    await safeDirectories(plan.root, plan.cargoDirectory);
    if (plan.target !== "server") {
      await safeDirectories(plan.root, path.join(plan.cargoDirectory, "wasm"));
      await safeDirectories(plan.root, path.dirname(plan.wasmPackage));
    }
    for (const command of plan.commands.slice(1)) {
      process.stdout.write(`[build ${plan.target}] jobs=${plan.jobs} (cd ${command.cwd} && ${[command.command, ...command.args].join(" ")})\n`);
      if (command.action === "stage-frontend") await stageFrontend(plan);
      else await commandRunner(command);
    }
    await publishArtifact({ root: plan.root, target: plan.target, host: plan.host, output: plan.artifact.directory, stageDirectory: path.join(plan.work, "artifact"),
      executable: path.join(plan.cargoDirectory, plan.nativeTarget, "release", plan.host === "windows" ? "server.exe" : "server"),
      webRoot: path.join(plan.frontend, "dist"), bundleRoot: path.join(plan.cargoDirectory, plan.nativeTarget, "release/bundle") });
    process.stdout.write(`[build ${plan.target}] published ${plan.artifact.directory}\n`);
  } finally {
    await cleanupOwnedDirectories(owned);
  }
}

export async function superviseBuild(plan, arguments_, { run = runBoundedCommand, timeoutMs = deadlineMs } = {}) {
  if (!Number.isInteger(timeoutMs) || timeoutMs < 6 || timeoutMs > deadlineMs) throw new Error("build deadline must be within 300000ms.");
  const started = performance.now();
  const reserveMs = Math.min(1000, Math.floor(timeoutMs / 3));
  const hardTimer = setTimeout(() => {
    process.stderr.write(`[build] total ${timeoutMs}ms deadline exhausted; cleanup not confirmed: ${plan.work}, ${plan.wasmRoot}\n`);
    process.exit(1);
  }, timeoutMs);
  let owned = [];
  try {
    owned = await prepareOwnedDirectories(plan);
    const remaining = Math.floor(timeoutMs - reserveMs - (performance.now() - started));
    if (remaining <= 1) throw new Error("build preparation exhausted its shared execution deadline.");
    await run({ command: process.execPath, args: [path.join(plan.root, "scripts/build-targets-worker.mjs"), ...arguments_], cwd: plan.root, timeoutMs: remaining,
      cleanupReserveMs: Math.min(reserveMs, Math.max(1, Math.floor(remaining / 3))),
      env: compilerEnvironment({ STOCK_TARGET_BUILD_ID: plan.buildId }) });
  } finally {
    try { await cleanupOwnedDirectories(owned); } finally { clearTimeout(hardTimer); }
  }
}

function printPlan(plan, dryRun, output) {
  output(`Target: ${plan.target}; native host: ${plan.host}; jobs=${plan.jobs}; shared deadline=300000ms.\n${plan.requirements}\n`);
  output("Native target: <rustc-host> is resolved from rustc -vV at execution; explicit --target overrides Cargo build.target configuration. Dry-run does not probe the Rust toolchain.\n");
  for (const command of plan.commands) output(`Command: ${Object.entries(command.env).map(([name, value]) => `${name}=${JSON.stringify(value)}`).join(" ")} (cd ${JSON.stringify(command.cwd)} && ${[command.command, ...command.args].map((value) => /\s/.test(value) ? JSON.stringify(value) : value).join(" ")})\n`);
  output(`Artifact: ${plan.artifact.directory}: ${plan.artifact.files.join(" + ")}\n`);
  if (plan.target !== "desktop") output(`Start: ${path.join(plan.artifact.directory, plan.artifact.files[0])} ${plan.artifact.startArgs.join(" ")}${plan.target === "server" ? "" : " (default --web-root: executable directory/webui)"}\n`);
  output(`Default existing outputs are refused; --output target/build-artifacts/NAME creates a new clean package.\nMode: ${dryRun ? "dry-run (no build commands executed; no artifact created)" : "supervised build; compilation only, CI regression unchanged"}.\n`);
}

export async function main(arguments_, { root = rootDirectory, host = hostName(), output = (message) => process.stdout.write(message), run = runBoundedCommand } = {}) {
  const options = parseBuildArgs(arguments_);
  if (options.help) {
    output("Usage: scripts/build.sh|build.bat <desktop|webui|webui-server|server> [--jobs N] [--dry-run] [--output target/build-artifacts/NAME]\nServer shell/batch routes need no Node. Native bundles only; build deadline 300000ms; no regression coupling.\n"); return;
  }
  const buildId = randomUUID();
  const plan = createBuildPlan(options, { root, host, buildId });
  printPlan(plan, options.dryRun, output);
  if (!options.dryRun) await superviseBuild(plan, arguments_, { run });
}

if (process.argv[1] !== undefined && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch((error) => { process.stderr.write(`[build] ${error.message}\n`); process.exitCode = 1; });
}
