import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import * as parity from "./host-parity.mjs";

test("Given missing artifact provenance When publication validates Then nominal success fails closed", async () => {
  assert.equal(typeof parity.assertPublicationFreshness, "function");
  await assert.rejects(() => parity.assertPublicationFreshness({ hashes: {}, inputs: {} }, "/run"), /missing required artifact/);
});

for (const artifact of ["generated-wasm", "tauri-default", "tauri-feature"]) {
  test(`Given a changed ${artifact} When a nominally successful run publishes Then freshness rejects it`, async () => {
    const directory = await mkdtemp(join(tmpdir(), "parity-freshness-"));
    const path = join(directory, artifact);
    const expected = createHash("sha256").update("current build output").digest("hex");
    try {
      await writeFile(path, "stale or replaced build output");
      assert.equal(typeof parity.assertArtifactHashes, "function");
      await assert.rejects(() => parity.assertArtifactHashes({ [path]: expected }), /artifact changed/);
    } finally {
      await rm(directory, { recursive: true, force: true });
    }
  });
}

test("Given an unchanged build output When freshness verifies Then its exact bytes are accepted", async () => {
  const directory = await mkdtemp(join(tmpdir(), "parity-freshness-"));
  const path = join(directory, "current-wasm");
  try {
    await writeFile(path, "current build output");
    const expected = createHash("sha256").update("current build output").digest("hex");
    assert.equal(typeof parity.assertArtifactHashes, "function");
    await parity.assertArtifactHashes({ [path]: expected });
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
