import assert from "node:assert/strict"
import { gzipSync } from "node:zlib"
import test from "node:test"
import { getInitialSaveConfiguration, type InitialSaveConfiguration, type InitialSaveSelection } from "./initial-save-selection.ts"
import { InitialSaveSource } from "./session-replacement.ts"
import { currentSaveFixture, withSettledDay } from "./current-save-fixture.ts"
import { parseSaveJson } from "./save-schema.ts"
import { createValidatedArchiveLease, inspectValidatedArchiveLease } from "./validated-archive-lease.ts"
import { captureGzipArchiveSource } from "./archive-source.ts"
import type { SessionSetup } from "../types/engine.ts"

function configurationTypes(value: InitialSaveConfiguration): { setup: SessionSetup; seed: string } { return value }

test("archive和lease分别从权威来源取得确切setup/seed，不缓存另一份冲突配置", { timeout: 10000 }, async () => {
  const text = JSON.stringify(withSettledDay(currentSaveFixture(), "2030-01-01", "2030-01-02")), archive = parseSaveJson(text)
  const lease = await createValidatedArchiveLease(captureGzipArchiveSource(`gzip:${gzipSync(text).toString("base64")}`))
  const selections: InitialSaveSelection[] = [{ kind: "archive", archive }, { kind: "lease", lease }]
  for (const selection of selections) {
    const result = configurationTypes(getInitialSaveConfiguration(selection))
    assert.deepEqual(result.setup, archive.setup); assert.equal(result.seed, "42"); assert.equal(typeof result.seed, "string")
  }
  const selected = getInitialSaveConfiguration({ kind: "lease", lease })
  assert.equal(selected.setup, inspectValidatedArchiveLease(lease).setup)
  assert.equal(Object.isFrozen(selected.setup), true)
  const ordinary = getInitialSaveConfiguration({ kind: "archive", archive })
  assert.equal(ordinary.setup, archive.setup)
  assert.equal(Object.isFrozen(archive.setup), false, "配置投影不改变原String archive可变契约")
})

test("新union可复用InitialSaveSource单次缓存与显式新局语义", { timeout: 10000 }, async () => {
  const source = new InitialSaveSource<InitialSaveSelection>(), archive = parseSaveJson(JSON.stringify(currentSaveFixture()))
  const selected: InitialSaveSelection = { kind: "archive", archive }
  let reads = 0
  assert.equal(await source.read(async () => { reads++; return selected }), selected)
  assert.equal(await source.read(async () => { throw new Error("不得重读") }), selected)
  assert.equal(reads, 1)
  source.reset(); assert.equal(await source.read(async () => { throw new Error("明确新局不得读槽") }), null)
})

test("lease结构伪造和未知来源显式拒绝，不fallback到archive字段", { timeout: 10000 }, () => {
  assert.throws(() => getInitialSaveConfiguration({ kind: "lease", lease: { kind: "validated-archive-lease", productionAdmission: false } }), /租约身份/)
  assert.throws(() => getInitialSaveConfiguration({ kind: "unknown" } as never), /初始存档来源/)
})
