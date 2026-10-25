// W0 ABI/feature隔离用dev harness；正式release product已经单独产出，不在此重编。
import { spawn, spawnSync } from "node:child_process";
import { appendFileSync, readFileSync } from "node:fs";
const env = { ...process.env, CARGO_BUILD_JOBS:"16", CARGO_TARGET_DIR:"/data1/baiyifan/workplace/stock_market_game/.tmp/wasm-w0-recovery/wasm-target", RUSTUP_TOOLCHAIN:"nightly-2026-09-05", CARGO_PROFILE_DEV_DEBUG:"0" };
const args = ["build", "apps/web-wasm", "--target", "web", "--dev", "--out-dir", "/data1/baiyifan/workplace/stock_market_game/.tmp/wasm-w0-recovery/artifacts/harness-dev/pkg", "--features", "verification-harness"];
console.log(JSON.stringify({kind:"dev-harness-ABI",args,jobs:16,debug:0,started:new Date().toISOString(),pgid:process.pid}));
function sample() {
  const result = spawnSync("ps", ["-eo", "pid,ppid,pgid,stat,pcpu,nlwp,comm"], { encoding:"utf8" });
  if (result.status !== 0) throw new Error(`CPU采样失败${result.status}`);
  const rows = result.stdout.split("\n").map((line) => line.trim().split(/\s+/)).filter((row) => row[2] === String(process.pid));
  appendFileSync("agents/branch-completion/wasm-w0/harness-dev-cpu.log", JSON.stringify({at:new Date().toISOString(),rows,io:readFileSync("/proc/pressure/io","utf8")})+"\n");
}
const child = spawn("wasm-pack",args,{env,stdio:"inherit"});
sample();const timer=setInterval(sample,2000);
try { await new Promise((resolve,reject)=>{child.on("error",reject);child.on("close",(code)=>code===0?resolve():reject(new Error(`dev harness退出${code}`)));}); }
finally { clearInterval(timer);sample(); }
console.log(JSON.stringify({completed:new Date().toISOString()}));
