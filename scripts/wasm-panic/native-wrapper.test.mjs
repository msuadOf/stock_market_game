import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);
const run = (file, args, env) => new Promise((resolve) => execFile(file, args, { env, encoding: "utf8" }, (error, stdout, stderr) => resolve({ status: error ? error.code : 0, stdout, stderr })));
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

test("原生 wrapper 保留参数、固定 sysroot 并传播退出状态", { timeout: 10000 }, async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "wasm native wrapper "));
  try {
    const rustc = (await execFileAsync("rustup", ["which", "--toolchain", "nightly-2026-09-05", "rustc"])).stdout.trim();
    const suffix = process.platform === "win32" ? ".exe" : "";
    const wrapper = path.join(root, `wrapper${suffix}`);
    const fixture = path.join(root, `fixture${suffix}`);
    const source = path.join(root, "fixture.rs");
    await writeFile(source, 'fn main() { for arg in std::env::args_os().skip(1) { println!("{:?}", arg); } if std::env::var_os("FAIL_FIXTURE").is_some() { std::process::exit(42); } }');
    await execFileAsync(rustc, [fileURLToPath(new URL("./rustc-wrapper.rs", import.meta.url)), "-o", wrapper]);
    await execFileAsync(rustc, [source, "-o", fixture]);
    const sysroot = path.join(root, "独立 sysroot");
    const env = { ...process.env, WASM_PANIC_SYSROOT: sysroot };
    const valid = await run(wrapper, [fixture, "a b", "quote\" & | %!", ""], env);
    assert.equal(valid.status, 0, valid.stderr);
    assert.deepEqual(valid.stdout.trimEnd().split(/\r?\n/), ['"--sysroot"', JSON.stringify(sysroot), '"a b"', '"quote\\\" & | %!"', '""']);
    for (const arg of ["--sysroot", "--sysroot=other"]) assert.notEqual((await run(wrapper, [fixture, arg], env)).status, 0);
    assert.notEqual((await run(wrapper, [fixture], { ...env, WASM_PANIC_SYSROOT: "" })).status, 0);
    assert.notEqual((await run(wrapper, [], env)).status, 0);
    assert.equal((await run(wrapper, [fixture], { ...env, FAIL_FIXTURE: "1" })).status, 42);
  } finally { await rm(root, { recursive: true, force: true }); }
});
