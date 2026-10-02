import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, mkdir, writeFile, readFile, rm, symlink } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";

async function fixture(root) {
  const targets = ["x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc", "aarch64-apple-darwin"];
  for (const product of ["desktop", "server", "webui-server", "web"]) {
    for (const target of product === "web" ? ["static"] : targets) {
      const directory = path.join(root, `stock-market-game-${product}-${target}-unsigned`);
      await mkdir(directory);
      const name = `${product}-${target}.zip`;
      const bytes = Buffer.from(`${product}/${target}`);
      await writeFile(path.join(directory, name), bytes);
      await writeFile(path.join(directory, "manifest.json"), JSON.stringify({ schema: "distribution-manifest-v1", product, target,
        ...(product === "web" ? { commit: "a".repeat(40) } : {}),
        files: [{ name, bytes: bytes.length, sha256: createHash("sha256").update(bytes).digest("hex") }] }));
    }
  }
}

test("publishing verifies all ten products, rejects tampering, and gives manifests collision-free names with source SHA", async () => {
  const { collectReleaseAssets } = await import("./publish-release.mjs");
  const root = await mkdtemp(path.join(tmpdir(), "release-assets-"));
  try {
    const input = path.join(root, "input");
    await mkdir(input);
    await fixture(input);
    const sha = "a".repeat(40);
    const assets = await collectReleaseAssets(input, path.join(root, "output"), sha);
    assert.equal(new Set(assets.map((file) => path.basename(file))).size, 21);
    const provenance = JSON.parse(await readFile(path.join(root, "output/release-source.json"), "utf8"));
    assert.equal(provenance.commit, sha);
    assert.equal(provenance.distributions.length, 10);
    const webManifest = path.join(input, "stock-market-game-web-static-unsigned/manifest.json");
    const web = JSON.parse(await readFile(webManifest, "utf8"));
    for (const commit of [undefined, "bad", "b".repeat(40)]) {
      await writeFile(webManifest, JSON.stringify({ ...web, commit }));
      await assert.rejects(collectReleaseAssets(input, path.join(root, "stale-output"), sha), /source SHA/);
    }
    await writeFile(webManifest, JSON.stringify(web));
    await writeFile(path.join(input, "stock-market-game-server-aarch64-apple-darwin-unsigned/server-aarch64-apple-darwin.zip"), "tampered");
    await assert.rejects(collectReleaseAssets(input, path.join(root, "bad-output"), sha), /digest|bytes/);
    await rm(path.join(input, "stock-market-game-desktop-aarch64-apple-darwin-unsigned"), { recursive: true });
    await assert.rejects(collectReleaseAssets(input, path.join(root, "missing-output"), sha), /ten|missing/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("upload failures leave the Release draft, while successful test tags publish as prereleases", async () => {
  const { main } = await import("./publish-release.mjs");
  const env = { RELEASE_TAG: "test-abc", RELEASE_SHA: "a".repeat(40), GITHUB_REPOSITORY: "owner/game" };
  const calls = [];
  const collect = async (input, output, sha) => {
    assert.equal(input, "target/release-input");
    assert.equal(output, "target/release-assets");
    assert.equal(sha, env.RELEASE_SHA);
    return ["target/release-assets/web-static.zip"];
  };
  await assert.rejects(main(env, async (args) => {
    calls.push(args);
    if (args[0] === "api") return env.RELEASE_SHA;
    throw new Error("upload failed");
  }, collect), /upload failed/);
  assert.equal(calls.length, 2);
  assert.ok(calls[1].includes("--draft") && calls[1].includes("--prerelease"));
  calls.length = 0;
  await main(env, async (args) => { calls.push(args); return env.RELEASE_SHA; }, collect);
  assert.equal(calls[2][1], "edit");
  assert.ok(calls[2].includes("--draft=false"));
});

test("publishing refuses symlink assets, collisions and a tag moved after compilation", async () => {
  const { collectReleaseAssets, main } = await import("./publish-release.mjs");
  const root = await mkdtemp(path.join(tmpdir(), "release-reject-"));
  try {
    const input = path.join(root, "input");
    await mkdir(input);
    await fixture(input);
    const directory = path.join(input, "stock-market-game-web-static-unsigned");
    const file = path.join(directory, "web-static.zip");
    await rm(file);
    await writeFile(path.join(root, "unrelated"), "web/static");
    await symlink(path.join(root, "unrelated"), file);
    await assert.rejects(collectReleaseAssets(input, path.join(root, "linked-output"), "a".repeat(40)), /asset bytes/);
    await rm(file);
    await writeFile(file, "web/static");
    const manifestFile = path.join(directory, "manifest.json");
    const manifest = JSON.parse(await readFile(manifestFile, "utf8"));
    const name = "server-aarch64-apple-darwin.zip";
    await writeFile(path.join(directory, name), "web/static");
    await rm(file);
    await writeFile(manifestFile, JSON.stringify({ ...manifest, files: [{ ...manifest.files[0], name }] }));
    await assert.rejects(collectReleaseAssets(input, path.join(root, "collision-output"), "a".repeat(40)), /collision/);
    const calls = [];
    await assert.rejects(main({ RELEASE_TAG: "test-abc", RELEASE_SHA: "a".repeat(40), GITHUB_REPOSITORY: "owner/game" }, async (args) => {
      calls.push(args); return "b".repeat(40);
    }), /tag commit changed/);
    assert.equal(calls.length, 1);
    assert.equal(calls[0][0], "api");
  } finally { await rm(root, { recursive: true, force: true }); }
});
