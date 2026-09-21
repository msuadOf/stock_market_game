import assert from "node:assert/strict";
import { mkdir, mkdtemp, rm, symlink, writeFile } from "node:fs/promises";
import path from "node:path";
import { test } from "node:test";
import { escrowSourceManifest } from "./escrow-source-manifest.mjs";

async function fixture(context) {
  const temporary = process.env.TMPDIR;
  assert.ok(temporary, "tests require workspace-local TMPDIR");
  const root = await mkdtemp(path.join(temporary, "escrow-source-manifest-"));
  context.after(() => rm(root, { recursive: true, force: true }));
  await Promise.all([
    mkdir(path.join(root, "apps/server/src"), { recursive: true }),
    mkdir(path.join(root, "docs"), { recursive: true }),
    mkdir(path.join(root, ".tmp/build-cache"), { recursive: true }),
  ]);
  await Promise.all([
    writeFile(path.join(root, "apps/server/src/main.rs"), "fn main() {}\n"),
    writeFile(path.join(root, "docs/architecture.md"), "# architecture\n"),
    writeFile(path.join(root, ".tmp/build-cache/transient"), "first\n"),
  ]);
  return root;
}

test("complete source manifest binds host source and docs but excludes workspace caches", async (context) => {
  const root = await fixture(context);
  const first = await escrowSourceManifest(root);
  assert(first.files.some(({ path: file }) => file === "apps/server/src/main.rs"));
  assert(first.files.some(({ path: file }) => file === "docs/architecture.md"));
  assert(!first.files.some(({ path: file }) => file.startsWith(".tmp/")));

  await writeFile(path.join(root, ".tmp/build-cache/transient"), "second\n");
  assert.equal((await escrowSourceManifest(root)).sha256, first.sha256);

  await writeFile(path.join(root, "apps/server/src/main.rs"), "fn main() { panic!() }\n");
  const hostChanged = await escrowSourceManifest(root);
  assert.notEqual(hostChanged.sha256, first.sha256);

  await writeFile(path.join(root, "docs/architecture.md"), "# changed\n");
  assert.notEqual((await escrowSourceManifest(root)).sha256, hostChanged.sha256);
});

test("complete source manifest refuses a symlinked included input", async (context) => {
  const root = await fixture(context);
  await symlink(path.join(root, "docs/architecture.md"), path.join(root, "docs/linked.md"));
  await assert.rejects(escrowSourceManifest(root), /refuses symlink/);
});
