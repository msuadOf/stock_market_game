import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdtemp, mkdir, writeFile, readFile, rm, symlink } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { packageStaticWeb } from "./package-static-web.mjs";

// 仅作为 packaging 前置的有效 release WASM，不证明真实 World 构建结果。
function releaseWasmFixture() {
  const section = (id, payload) => [id, payload.length, ...payload];
  const name = (value) => [value.length, ...Buffer.from(value)];
  const type = [0x01, 0x08, 0x02, 0x60, 0x00, 0x00, 0x60, 0x00, 0x01, 0x7c];
  const imports = section(0x02, [0x01, ...name("env"), ...name("engine_clock_now"), 0x00, 0x01]);
  const functions = section(0x03, [0x01, 0x00]);
  const code = section(0x0a, [0x01, 0x05, 0x00, 0xfe, 0x03, 0x00, 0x0b]);
  return Buffer.from([0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, ...type, ...imports, ...functions, ...code]);
}

test("static Web packager refuses invalid source provenance and arbitrary paths before touching files", async () => {
  await assert.rejects(packageStaticWeb({ input: "apps/web/dist-pages", output: "target/distributions/web", sha: "bad" }), /SHA/);
  await assert.rejects(packageStaticWeb({ input: "../other", output: "target/distributions/web", sha: "a".repeat(40) }), /packaging requires/);
});

test("static archives carry source metadata and license, reject reruns and cannot publish through symlink parents", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "static-web-"));
  const options = { input: "apps/web/dist-pages", output: "target/distributions/web", sha: "a".repeat(40) };
  try {
    const input = path.join(root, options.input);
    await mkdir(input, { recursive: true });
    await writeFile(path.join(root, "LICENSE"), "MIT");
    await writeFile(path.join(input, "index.html"), "<script src='app.js'></script>");
    await writeFile(path.join(input, "pages-isolation.js"), "isolation worker");
    await writeFile(path.join(input, "web_wasm_bg.wasm"), releaseWasmFixture());
    const unrelated = path.join(root, "unrelated");
    await mkdir(unrelated);
    await mkdir(path.join(root, "target"));
    await symlink(unrelated, path.join(root, "target/distributions"), process.platform === "win32" ? "junction" : "dir");
    await assert.rejects(packageStaticWeb(options, { root }), /symlink/);
    await assert.rejects(readFile(path.join(unrelated, "web/manifest.json")), /ENOENT/);
    await rm(path.join(root, "target/distributions"));
    const archives = [];
    const manifest = await packageStaticWeb(options, { root, archive: async (archive) => {
      archives.push(archive);
      await writeFile(archive.output, `archive/${archive.format}`);
    } });
    assert.equal(manifest.schema, "distribution-manifest");
    assert.equal(manifest.schema_version, 1);
    assert.equal(manifest.commit, options.sha);
    assert.deepEqual(manifest.files.map((file) => file.name), ["web-static.zip", "web-static.tar.gz"]);
    assert.equal(JSON.parse(await readFile(path.join(input, "build-info.json"), "utf8")).commit, options.sha);
    assert.ok(archives.every((archive) => archive.entries.includes("LICENSE") && archive.entries.includes("build-info.json")));
    await assert.rejects(packageStaticWeb(options, { root }), /output already/);
  } finally { await rm(root, { recursive: true, force: true }); }
});
