import { createHash } from "node:crypto";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { cp, mkdir, readFile, readdir, rm, symlink, writeFile } from "node:fs/promises";
import path from "node:path";
import { patchPanicSource } from "./patch-std.mjs";

const PINNED_COMMIT = "0ed41eb4142dda2df61eb1145a312c1a9d62eb56";
const PINNED_PANIC_HASH = "f4e1965ebceb1f86132610c5634695f6fe1912e43f59a61f85aabda634eb0ead";
const PINNED_TREE_HASH = "541c4409d97ac4c8817fce28f6036b0dadcecbd3fe582271ac0e00a6787fed13";

/** 验证工具链 commit，不能以调用方提供的哈希代替固定版本。 */
export function pinnedRustcHost(version) {
  const commits = [...version.matchAll(/^commit-hash: (.+)$/gm)];
  const hosts = [...version.matchAll(/^host: (.+)$/gm)];
  if (commits.length !== 1 || commits[0][1] !== PINNED_COMMIT || hosts.length !== 1 || !/^[a-z0-9_]+-[a-z0-9_-]+$/.test(hosts[0][1])) {
    throw new Error("rustc 版本或 host 与锁定工具链不一致");
  }
  return hosts[0][1];
}

/** 生产入口同时锁定 rustc commit、panic 源文件和完整 rust-src。 */
export async function preparePinnedSysroot(rustc, destination) {
  const run = promisify(execFile);
  const version = await run(rustc, ["-vV"]);
  const host = pinnedRustcHost(version.stdout);
  const sysroot = await run(rustc, ["--print", "sysroot"]);
  await prepareSysroot(sysroot.stdout.trim(), destination, PINNED_PANIC_HASH, host, PINNED_TREE_HASH);
}

/** 按相对路径及文件字节计算完整 rust-src 库哈希；拒绝符号链接和特殊文件。 */
export async function hashSourceTree(root) {
  const hash = createHash("sha256");
  async function visit(relative) {
    const entries = await readdir(path.join(root, relative), { withFileTypes: true });
    entries.sort((a, b) => a.name < b.name ? -1 : a.name > b.name ? 1 : 0);
    for (const entry of entries) {
      const name = relative === "" ? entry.name : `${relative}/${entry.name}`;
      if (entry.isDirectory()) await visit(name);
      else if (entry.isFile()) {
        const data = await readFile(path.join(root, name));
        hash.update(`${Buffer.byteLength(name)}:${name}:${data.length}:`);
        hash.update(data);
      } else throw new Error(`rust-src 包含不支持的文件类型：${name}`);
    }
  }
  await visit("");
  return hash.digest("hex");
}

/** 创建独立 std 副本；版本不匹配或输出已存在均显式失败。 */
export async function prepareSysroot(sourceSysroot, destination, expectedHash, hostTriple, expectedTreeHash) {
  if (!/^[a-z0-9_]+-[a-z0-9_-]+$/.test(hostTriple)) throw new Error("host triple 非法");
  const sourceRoot = path.resolve(sourceSysroot, "lib/rustlib");
  const sourceLibrary = path.join(sourceRoot, "src/rust/library");
  const sourceFile = path.join(sourceLibrary, "std/src/panicking.rs");
  const treeHash = await hashSourceTree(sourceLibrary);
  if (treeHash !== expectedTreeHash) throw new Error(`rust-src 全库哈希不匹配：${treeHash}`);
  const source = await readFile(sourceFile, "utf8");
  const hash = createHash("sha256").update(source).digest("hex");
  if (hash !== expectedHash) throw new Error(`std panic 源码哈希不匹配：${hash}`);
  const patched = patchPanicSource(source);
  const target = path.resolve(destination);
  await mkdir(target);
  try {
    const targetRustlib = path.join(target, "lib/rustlib");
    const targetLibrary = path.join(targetRustlib, "src/rust/library");
    await mkdir(path.dirname(targetLibrary), { recursive: true });
    await cp(sourceLibrary, targetLibrary, { recursive: true, errorOnExist: true, force: false });
    await writeFile(path.join(targetLibrary, "std/src/panicking.rs"), patched);
    for (const triple of new Set([hostTriple, "wasm32-unknown-unknown"])) {
      await symlink(path.join(sourceRoot, triple), path.join(targetRustlib, triple), "junction");
    }
  } catch (error) {
    await rm(target, { recursive: true, force: true });
    throw error;
  }
}
