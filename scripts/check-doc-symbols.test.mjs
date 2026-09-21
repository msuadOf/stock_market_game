import assert from "node:assert/strict";
import { mkdtemp, mkdir, writeFile, rm } from "node:fs/promises";
import path from "node:path";
import { test } from "node:test";
import { checkDocSymbols, referencedEngineSymbols } from "./check-doc-symbols.mjs";

test("extracts explicit engine references without mistaking UI names or prose for engine API", () => {
  assert.deepEqual(referencedEngineSymbols("`engine::pipeline::seal_allocation_snapshot` `EngineHost` <!-- engine-symbol: StrategyState -->"),
    ["StrategyState", "seal_allocation_snapshot"]);
});

test("reports a nonexistent documented symbol and refuses empty false PASS", async () => {
  assert(process.env.TMPDIR, "test requires workspace-local TMPDIR");
  const root = await mkdtemp(path.join(process.env.TMPDIR, "doc-symbol-test-"));
  try {
    await mkdir(path.join(root, "packages/engine/src"), { recursive: true });
    await writeFile(path.join(root, "packages/engine/src/lib.rs"), "pub fn real_symbol() {}\n");
    await writeFile(path.join(root, "doc.md"), "`engine::real_symbol`\n`engine::invented_symbol`\n");
    const result = await checkDocSymbols(root, ["doc.md"]);
    assert.equal(result.status, "FAIL");
    assert.deepEqual(result.missing, [{ document: "doc.md", symbol: "invented_symbol" }]);
    await writeFile(path.join(root, "doc.md"), "`engine::real_symbol`\n");
    assert.equal((await checkDocSymbols(root, ["doc.md"])).status, "PASS");
    await writeFile(path.join(root, "doc.md"), "No annotated engine symbols.\n");
    assert.equal((await checkDocSymbols(root, ["doc.md"])).status, "FAIL");
    await assert.rejects(checkDocSymbols(root, ["../outside.md"]), /escapes/);
  } finally { await rm(root, { recursive: true }); }
});
