// W0 定向长构建；由进程外 run-long-validation 统一限制共享300000ms。
import { spawn } from "node:child_process";
import { createWriteStream } from "node:fs";
import { availableParallelism } from "node:os";
import path from "node:path";
const cwd = process.cwd();
const evidence = path.join(cwd, "agents/branch-completion/wasm-w0");
const target = "/data1/baiyifan/workplace/stock_market_game/.tmp/wasm-w0-recovery";
const artifact = "/data1/baiyifan/workplace/stock_market_game/.tmp/wasm-w0-recovery/artifacts";
console.log(JSON.stringify({ jobs: 16, available: availableParallelism(), target, artifact, started: new Date().toISOString() }));
async function run(label, command, args, env = {}) {
  const output = createWriteStream(path.join(evidence, `${label}.log`));
  const child = spawn(command, args, { cwd, env: { ...process.env, CARGO_BUILD_JOBS: "16", CARGO_TARGET_DIR: path.join(target, label === "native-check" ? "native-target" : "wasm-target"), ...env }, stdio: ["ignore", "pipe", "pipe"] });
  child.stdout.pipe(output, { end: false }); child.stderr.pipe(output, { end: false });
  console.log(JSON.stringify({ label, pid: child.pid, command, args }));
  await new Promise((resolve, reject) => { child.on("error", reject); child.on("close", (code) => code === 0 ? resolve() : reject(new Error(`${label} exit ${code}`))); });
  output.end();
  console.log(JSON.stringify({ label, completed: new Date().toISOString() }));
}
const results = await Promise.allSettled([
  run("native-check", "cargo", ["check", "-p", "engine", "-p", "web-wasm", "--lib", "-j", "16"]),
  (async () => {
    await run("wasm-product-build", "wasm-pack", ["build", "apps/web-wasm", "--target", "web", "--release", "--out-dir", `${artifact}/product/pkg`], { RUSTUP_TOOLCHAIN: "nightly-2026-09-05" });
    await run("wasm-harness-build", "wasm-pack", ["build", "apps/web-wasm", "--target", "web", "--release", "--out-dir", `${artifact}/harness/pkg`, "--features", "verification-harness"], { RUSTUP_TOOLCHAIN: "nightly-2026-09-05" });
  })(),
]);

for (const result of results) { if (result.status === "rejected") throw result.reason; }
