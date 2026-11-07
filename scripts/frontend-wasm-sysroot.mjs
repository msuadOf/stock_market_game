import { execFile } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { promisify } from "node:util";
import { preparePinnedSysroot } from "./wasm-panic/prepare-sysroot.mjs";

const execFileAsync = promisify(execFile);

async function resolvePinnedRustc() {
  const { stdout } = await execFileAsync("rustup", ["which", "--toolchain", "nightly-2026-09-05", "rustc"]);
  const rustc = stdout.trim();
  if (!path.isAbsolute(rustc) || rustc.includes("\n")) throw new Error("rustup did not return an absolute rustc path");
  return rustc;
}

export async function withFrontendWasmSysroot(command, run, { tempRoot = tmpdir(), environment = process.env, resolveRustc = resolvePinnedRustc, prepare = preparePinnedSysroot, compile = execFileAsync, host = process.platform } = {}) {
  if (environment.RUSTC_WRAPPER || command.env.RUSTC_WRAPPER || environment.RUSTC_WORKSPACE_WRAPPER || command.env.RUSTC_WORKSPACE_WRAPPER) throw new Error("existing RUSTC_WRAPPER or RUSTC_WORKSPACE_WRAPPER cannot be combined with the controlled sysroot");
  const temporary = await mkdtemp(path.join(tempRoot, "stock-wasm-sysroot-"));
  try {
    const sysroot = path.join(temporary, "sysroot");
    const rustc = await resolveRustc();
    await prepare(rustc, sysroot);
    const wrapper = path.join(temporary, host === "win32" || host === "windows" ? "rustc-wrapper.exe" : "rustc-wrapper");
    await compile(rustc, [path.join(import.meta.dirname, "wasm-panic/rustc-wrapper.rs"), "-o", wrapper]);
    return await run({ ...command, env: { ...command.env, RUSTC_WRAPPER: wrapper, WASM_PANIC_SYSROOT: sysroot } });
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
}
