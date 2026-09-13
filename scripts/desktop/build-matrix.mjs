#!/usr/bin/env node

import { existsSync, readFileSync, statSync } from "node:fs";
import { platform } from "node:os";
import { resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(import.meta.dirname, "../..");
const hosts = new Set(["linux", "macos", "windows"]);
const targets = ["linux", "macos", "windows"];
const tauriDirectory = resolve(repoRoot, "apps/desktop/src-tauri");

function fail(message) {
  throw new Error(message);
}

function usage() {
  return `usage: scripts/desktop/build-matrix.sh [--dry-run] [--host linux|macos|windows] [--target linux|macos|windows|all]\n\n--host simulates planning only and requires --dry-run.`;
}

function parseArguments(arguments_) {
  let dryRun = false;
  let host = null;
  let target = "all";

  for (let index = 0; index < arguments_.length; index += 1) {
    const argument = arguments_[index];
    if (argument === "--dry-run") {
      dryRun = true;
    } else if (argument === "--host" || argument === "--target") {
      const value = arguments_[index + 1];
      if (value === undefined || value.startsWith("--")) {
        fail(`${argument} requires a value\n${usage()}`);
      }
      index += 1;
      if (argument === "--host") {
        host = value;
      } else {
        target = value;
      }
    } else if (argument === "--help" || argument === "-h") {
      process.stdout.write(`${usage()}\n`);
      process.exit(0);
    } else {
      fail(`unknown argument: ${argument}\n${usage()}`);
    }
  }

  if (host !== null && !dryRun) {
    fail(`--host is only available with --dry-run; it simulates planning and cannot change a real build host.\n${usage()}`);
  }
  if (host !== null && !hosts.has(host)) {
    fail(`--host must be one of: linux, macos, windows\n${usage()}`);
  }
  if (target !== "all" && !hosts.has(target)) {
    fail(`--target must be one of: linux, macos, windows, all\n${usage()}`);
  }

  return { dryRun, host: host ?? detectHost(), target };
}

function detectHost() {
  const currentPlatform = platform();
  if (currentPlatform === "linux") return "linux";
  if (currentPlatform === "darwin") return "macos";
  if (currentPlatform === "win32") return "windows";
  fail(`unsupported runtime host: ${currentPlatform}; supported hosts are linux, macos, windows.`);
}

function planCell(host, target) {
  if (host === target) {
    if (target === "linux") {
      return {
        status: "supported",
        restriction: "Native Linux bundles only: deb, rpm, appimage.",
        prerequisites: "Preflight checks generated frontend/WASM artifacts, Node dependencies, Corepack-managed pnpm, cargo, and cargo-tauri availability. Linux GTK/WebKitGTK/pkg-config requirements and tool versions are deferred to the Tauri/Cargo build.",
        build: { command: "cargo", arguments: ["tauri", "build", "--bundles", "deb,rpm,appimage"] },
      };
    }
    if (target === "macos") {
      return {
        status: "supported",
        restriction: "Native macOS bundles only: app, dmg. Signing and notarization remain macOS credential workflows.",
        prerequisites: "Preflight checks generated frontend/WASM artifacts, Node dependencies, Corepack-managed pnpm, cargo, and cargo-tauri availability. Xcode, signing, notarization, and tool versions are deferred to the Tauri/Cargo build.",
        build: { command: "cargo", arguments: ["tauri", "build", "--bundles", "app,dmg"] },
      };
    }
    return {
      status: "supported",
      restriction: "Native Windows bundles only: msi, nsis. MSI remains Windows-native and requires WiX/VBScript.",
      prerequisites: "Preflight checks generated frontend/WASM artifacts, Node dependencies, Corepack-managed pnpm, cargo, and cargo-tauri availability. Windows WiX/VBScript for MSI and tool versions are deferred to the Tauri/Cargo build.",
      build: { command: "cargo", arguments: ["tauri", "build", "--bundles", "msi,nsis"] },
    };
  }

  if (target === "windows" && (host === "linux" || host === "macos")) {
    return {
      status: "constrained",
        restriction: "NSIS-only experimental cross-build. cargo-xwin compiles the Rust Windows target; it does not make MSI or Windows-native WiX available. The build may be unsigned; signed distribution requires an external bundle.windows.signCommand because Tauri's default Windows signer runs only on Windows.",
        prerequisites: "Normal preflight checks generated frontend/WASM artifacts, Node dependencies, Corepack-managed pnpm, cargo, cargo-tauri, cargo-xwin, NSIS, the installed x86_64-pc-windows-msvc Rust target, the LLVM lld-link driver, and llvm-rc. LLVM/NSIS versions and external signing configuration are deferred to the Tauri/Cargo build.",
        build: { command: "cargo", arguments: ["tauri", "build", "--runner", "cargo-xwin", "--target", "x86_64-pc-windows-msvc", "--bundles", "nsis"] },
    };
  }

  const reason = target === "macos"
    ? "macOS application bundles require a Mac computer; this is not an officially documented Tauri packaging path."
    : "This is not an officially documented Tauri v2 packaging path; do not confuse a Rust --target compile with a Tauri installer bundle.";
  return {
    status: "unsupported",
    restriction: "No supported installer/bundle route.",
    prerequisites: "No toolchain is checked because this route is intentionally not executed.",
    build: null,
    reason,
  };
}

function printPlan(host, target, dryRun) {
  const plan = planCell(host, target);
  const checks = plan.status === "constrained"
    ? "frontend/WASM artifacts; node_modules; Corepack-managed pnpm; cargo; cargo-tauri; cargo-xwin; NSIS; MSVC target; LLVM linker; LLVM resource compiler."
    : plan.status === "supported"
      ? "frontend/WASM artifacts; node_modules; Corepack-managed pnpm; cargo; cargo-tauri."
      : "none.";
  const performedChecks = dryRun ? `none (dry-run); normal mode checks: ${checks}` : checks;
  process.stdout.write(`Host: ${host}\nTarget: ${target}\nTauri package: stock-market-game\nStatus: ${plan.status}\nBundle restriction: ${plan.restriction}\nRoute requirements: ${plan.prerequisites}\nPreflight checks performed: ${performedChecks}\n`);
  if (plan.build !== null) process.stdout.write(`Generated command: (cd apps/desktop/src-tauri && ${[plan.build.command, ...plan.build.arguments].join(" ")})\n`);
  if (plan.reason !== undefined) process.stdout.write(`Reason: ${plan.reason}\n`);
  process.stdout.write(`Mode: ${dryRun ? "dry-run (no commands were executed)" : "build"}\n\n`);
  return plan;
}

function requireFile(relativePath, remedy) {
  const path = resolve(repoRoot, relativePath);
  if (!existsSync(path) || !statSync(path).isFile() || statSync(path).size === 0) {
    fail(`missing ${relativePath}; ${remedy}`);
  }
}

function requireDirectory(relativePath, remedy) {
  const path = resolve(repoRoot, relativePath);
  if (!existsSync(path) || !statSync(path).isDirectory()) fail(`missing ${relativePath}; ${remedy}`);
}

function requireWasmThreadingContract() {
  const gluePath = resolve(repoRoot, "apps/web/wasm-pkg/web_wasm.js");
  const wasmPath = resolve(repoRoot, "apps/web/wasm-pkg/web_wasm_bg.wasm");
  try {
    new WebAssembly.Module(readFileSync(wasmPath));
  } catch {
    fail("generated frontend/WASM binary is invalid; rerun scripts/wasm-build.sh on Linux/macOS or scripts\\wasm-build.bat on Windows.");
  }
  const result = spawnSync(process.execPath, [resolve(repoRoot, "scripts/check-wasm-threading.mjs"), gluePath], { cwd: repoRoot, stdio: "ignore" });
  if (result.error !== undefined || result.status !== 0) {
    fail("generated frontend/WASM threading contract is invalid; rerun scripts/wasm-build.sh on Linux/macOS or scripts\\wasm-build.bat on Windows.");
  }
}

function runCommand(command, arguments_, options = {}) {
  const result = spawnSync(command, arguments_, { cwd: repoRoot, stdio: "ignore", ...options, shell: false });
  return { command, result };
}

function requireVersionCommand(command, remedy) {
  const probe = runCommand(command, ["--version"]);
  if (probe.result.error === undefined && probe.result.status === 0) return probe.command;
  fail(`${command} is required; ${remedy}`);
}

export function corepackProbe(runtimePlatform = platform(), commandRunner = runCommand) {
  const probe = runtimePlatform === "win32"
    ? commandRunner(process.env.ComSpec ?? "cmd.exe", ["/d", "/s", "/c", "corepack pnpm --version"])
    : commandRunner("corepack", ["pnpm", "--version"]);
  return probe.result.error === undefined && probe.result.status === 0;
}

function requireCorepack() {
  if (!corepackProbe()) fail("Corepack-managed pnpm is required; install or enable Corepack so Tauri can run its configured frontend command.");
}

function requireAvailableCommand(command, remedy) {
  const probe = runCommand(command, []);
  if (probe.result.error === undefined) return probe.command;
  fail(`${command} is required; ${remedy}`);
}

function requireRustTarget(target) {
  const result = spawnSync("rustup", ["target", "list", "--installed"], { cwd: repoRoot, encoding: "utf8", stdio: "pipe" });
  if (result.error !== undefined || result.status !== 0 || !result.stdout.split("\n").includes(target)) {
    fail(`${target} Rust target is required; run \`rustup target add ${target}\` before the NSIS-only cross-build.`);
  }
}

function requireLlvmResourceCompiler() {
  requireAvailableCommand("llvm-rc", "install LLVM so `llvm-rc` is available to compile the Windows icon resource.");
}

function verifyPrerequisites(plan) {
  requireFile("apps/web/wasm-pkg/web_wasm.js", "run scripts/wasm-build.sh on Linux/macOS or scripts\\wasm-build.bat on Windows to generate non-empty frontend/WASM assets.");
  requireFile("apps/web/wasm-pkg/web_wasm_bg.wasm", "run scripts/wasm-build.sh on Linux/macOS or scripts\\wasm-build.bat on Windows to generate non-empty frontend/WASM assets.");
  requireWasmThreadingContract();
  requireDirectory("node_modules", "run corepack pnpm install --frozen-lockfile to install the repository-pinned pnpm dependencies.");
  requireCorepack();
  const cargo = requireVersionCommand("cargo", "install Rust from rust-toolchain.toml.");
  const tauri = runCommand(cargo, ["tauri", "--version"]);
  if (tauri.result.error !== undefined || tauri.result.status !== 0) {
    fail("the cargo-tauri subcommand is required; install the Tauri CLI for this project. Do not assume a globally available `tauri` executable.");
  }
  if (plan.status === "constrained") {
    requireVersionCommand("cargo-xwin", "install cargo-xwin, add x86_64-pc-windows-msvc, and install NSIS plus LLVM/lld per Tauri's Windows cross-build documentation.");
    const nsis = runCommand("makensis", ["/CMDHELP"]);
    if (nsis.result.error !== undefined || nsis.result.status !== 0) fail("makensis is required; install NSIS before attempting the NSIS-only cross-build.");
    requireRustTarget("x86_64-pc-windows-msvc");
    requireAvailableCommand("lld-link", "install LLVM and LLD so `lld-link` is available for Tauri's Windows cross-build.");
    requireLlvmResourceCompiler();
  }
  return cargo;
}

function runBuild(plan) {
  if (plan.status === "unsupported") fail(plan.reason);
  const cargo = verifyPrerequisites(plan);
  if (plan.build === null) fail(plan.reason);
  const result = runCommand(cargo, plan.build.arguments, {
    cwd: tauriDirectory,
    stdio: "inherit",
  });
  if (result.result.error !== undefined) fail(`could not start cargo tauri build: ${result.result.error.message}`);
  if (result.result.status !== 0) process.exit(result.result.status ?? 1);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) try {
  const options = parseArguments(process.argv.slice(2));
  if (!options.dryRun && options.target === "all") {
    fail("normal builds require exactly one --target; use --dry-run --target all to inspect the full matrix.");
  }
  const selectedTargets = options.target === "all" ? targets : [options.target];
  const plans = selectedTargets.map((target) => printPlan(options.host, target, options.dryRun));
  if (!options.dryRun) {
    runBuild(plans[0]);
  }
} catch (error) {
  process.stderr.write(`desktop build matrix: ${error.message}\n`);
  process.exit(1);
}
