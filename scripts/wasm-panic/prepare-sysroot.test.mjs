import assert from "node:assert/strict";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { createHash } from "node:crypto";
import { prepareSysroot, hashSourceTree, pinnedRustcHost } from "./prepare-sysroot.mjs";

const source = `fn panic_with_hook(
    payload: &mut dyn PanicPayload,
    location: &'static Location<'static>,
    can_unwind: bool,
    force_no_backtrace: bool,
) -> ! {
    let must_abort = panic_count::increase(true);
}
`;
test("只修改新 sysroot 中固定哈希的 std 副本", { timeout: 10000 }, async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "wasm-sysroot-test-"));
  try {
    const original = path.join(root, "input", "lib", "rustlib");
    const stdDir = path.join(original, "src", "rust", "library", "std", "src");
    await mkdir(stdDir, { recursive: true });
    await mkdir(path.join(original, "x86_64-unknown-linux-gnu"));
    await mkdir(path.join(original, "wasm32-unknown-unknown"));
    const originalFile = path.join(stdDir, "panicking.rs");
    await writeFile(originalFile, source);
    const hash = createHash("sha256").update(source).digest("hex");
    const treeHash = await hashSourceTree(path.join(original, "src/rust/library"));
    const target = path.join(root, "output");
    await prepareSysroot(path.join(root, "input"), target, hash, "x86_64-unknown-linux-gnu", treeHash);
    assert.equal(await readFile(originalFile, "utf8"), source);
    assert.match(await readFile(path.join(target, "lib/rustlib/src/rust/library/std/src/panicking.rs"), "utf8"), /wasm_tombstone_early/);
    await assert.rejects(prepareSysroot(path.join(root, "input"), target, hash, "x86_64-unknown-linux-gnu", treeHash));
    await assert.rejects(prepareSysroot(path.join(root, "input"), path.join(root, "bad"), "0".repeat(64), "x86_64-unknown-linux-gnu", treeHash), /哈希/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("全库哈希覆盖 panic 文件以外的源码", { timeout: 10000 }, async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "wasm-sysroot-hash-"));
  try {
    await writeFile(path.join(root, "first"), "A");
    const before = await hashSourceTree(root);
    await writeFile(path.join(root, "second"), "B");
    assert.notEqual(await hashSourceTree(root), before);
    await writeFile(path.join(root, "second"), "C");
    assert.notEqual(await hashSourceTree(root), before);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("只接受锁定的 rustc commit 和有效 host", { timeout: 10000 }, () => {
  const text = "rustc 1.100.0-nightly\ncommit-hash: 0ed41eb4142dda2df61eb1145a312c1a9d62eb56\nhost: x86_64-unknown-linux-gnu\n";
  assert.equal(pinnedRustcHost(text), "x86_64-unknown-linux-gnu");
  assert.throws(() => pinnedRustcHost(text.replace("0ed41eb", "1111111")), /rustc/);
  assert.throws(() => pinnedRustcHost(text + "host: other\n"), /rustc/);
});
