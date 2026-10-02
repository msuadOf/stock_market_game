import path from "node:path";

export function createFrontendCommands({ root, host, jobs, frontend, wasmPackage, env, staged = false, packageManager = "corepack" }) {
  if (!Number.isSafeInteger(jobs) || jobs < 1) throw new Error("frontend jobs must be a positive integer.");
  if (!["linux", "windows", "macos"].includes(host)) throw new Error(`unsupported frontend host: ${host}`);
  if (!["corepack", "pnpm"].includes(packageManager)) throw new Error(`unsupported package manager: ${packageManager}`);
  const buildEnv = { ...env, CARGO_BUILD_JOBS: String(jobs), RAYON_NUM_THREADS: String(jobs) };
  const nodeCommand = (args, cwd = root) => ({ command: process.execPath, args, cwd, env: buildEnv });
  const installArgs = [...(packageManager === "corepack" ? ["pnpm"] : []), "install", "--frozen-lockfile"];
  const copiedPackage = path.join(frontend, "wasm-pkg");
  return [
    host === "windows"
      ? { command: "cmd.exe", args: ["/d", "/s", "/c", [packageManager, ...installArgs].join(" ")], cwd: root, env: buildEnv }
      : { command: packageManager, args: installArgs, cwd: root, env: buildEnv },
    { command: "wasm-pack", args: ["build", "apps/web-wasm", "--target", "web", "--release", "--out-dir", wasmPackage, "-Z", "build-std=panic_abort,std"], cwd: root,
      env: { ...buildEnv, CARGO_TARGET_DIR: staged ? path.join(env.CARGO_TARGET_DIR, "wasm") : env.CARGO_TARGET_DIR, RUSTUP_TOOLCHAIN: "nightly-2026-09-05" } },
    nodeCommand([path.join(root, "scripts/check-wasm-threading.mjs"), path.join(wasmPackage, "web_wasm.js")]),
    nodeCommand([path.join(root, "scripts/check-web-release-wasm.mjs"), wasmPackage]),
    { action: staged ? "stage-frontend" : "copy-wasm", command: staged ? "stage-frontend" : "copy-wasm", args: [frontend], cwd: root, env: buildEnv },
    nodeCommand([path.join(root, "scripts/check-wasm-threading.mjs"), path.join(copiedPackage, "web_wasm.js")]),
    nodeCommand([path.join(root, "apps/web/node_modules/typescript/bin/tsc"), "--build", "--force"], frontend),
    nodeCommand([path.join(root, "apps/web/node_modules/vite/bin/vite.js"), "build", "--outDir", path.join(frontend, "dist")], frontend),
    nodeCommand([path.join(root, "scripts/check-web-release-wasm.mjs"), path.join(frontend, "dist")]),
  ];
}
