// 复用既审 Cargo 域外输入封存、native guard、W1 build monitor 和 LICENSE 门。
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { spawn } from "node:child_process";
const [planFile, mode, flag] = process.argv.slice(2);
if (!["native", "tax", "wasm"].includes(mode) || (flag !== undefined && flag !== "--snapshot-only")) throw new Error("必须明确 native／tax／wasm 与可选只读输入快照");
const plan = JSON.parse(readFileSync(planFile, "utf8"));
const toolHost = plan.wasmToolHost;
const { cargoChildEnvironment, snapshotCargoInputs, verifyCargoInputs } = await import(pathToFileURL(path.join(toolHost, "agents/branch-completion/wasm-w1/combined/cargo-inputs.mjs")));
const cwd = mode === "wasm" ? path.join(plan.cwd, "apps/web-wasm") : plan.cwd;
const environment = cargoChildEnvironment(process.env, mode === "wasm" ? plan.wasmEnvironment : plan.environment, cwd);
const before = snapshotCargoInputs({ cargoCwd: cwd, env: environment });
if (flag === "--snapshot-only") {
  process.stdout.write(`${JSON.stringify(before)}\n`);
  process.exit(0);
}
verifyCargoInputs(mode === "wasm" ? plan.expectedWasmCargoInputs : plan.expectedNativeCargoInputs, before);
const out = path.dirname(planFile);
writeFileSync(path.join(out, `${mode}-cargo-inputs-before.json`), `${JSON.stringify(before, null, 2)}\n`, { flag: "wx" });
if (mode !== "wasm") {
  await new Promise((resolve, reject) => {
    const child = spawn(plan.node, [plan.nativeGuard, planFile], { cwd: plan.cwd, env: environment, stdio: "inherit" });
    child.once("error", reject);
    child.once("exit", (code, signal) => code === 0 ? resolve() : reject(new Error(`native guard失败：code=${code} signal=${signal}`)));
  });
} else {
  const { executeWasmPackBuild } = await import(pathToFileURL(path.join(plan.cwd, "scripts/wasm-pack-build.mjs")));
  const { runMonitoredBuild } = await import(pathToFileURL(path.join(toolHost, "agents/branch-completion/wasm-w1/combined/build-monitor.mjs")));
  const remainingMs = plan.sharedDeadlineUnixMs - Date.now();
  if (remainingMs <= 1000) throw new Error("WASM构建无原始共享工作预算");
  const args = ["build", "apps/web-wasm", "--target", "web", "--dev", "--out-dir", plan.wasmArtifact, "--", "--features", "verification-harness", "--message-format=json-render-diagnostics"];
  await executeWasmPackBuild(plan.cwd, { command: "wasm-pack", args, cwd: plan.cwd }, () => runMonitoredBuild({
    command: "wasm-pack", args, cwd: plan.cwd, env: environment,
    executionDeadlineNs: String(process.hrtime.bigint() + BigInt(remainingMs) * 1_000_000n),
    idleTimeoutMs: 60000, sampleIntervalMs: 500,
    receiptPath: path.join(out, "wasm-build-monitor.json"),
  }));
}
const after = snapshotCargoInputs({ cargoCwd: cwd, env: environment });
verifyCargoInputs(before, after);
writeFileSync(path.join(out, `${mode}-cargo-inputs-after.json`), `${JSON.stringify(after, null, 2)}\n`, { flag: "wx" });
