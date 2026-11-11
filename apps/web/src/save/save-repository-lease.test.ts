import assert from "node:assert/strict"
import { setImmediate } from "node:timers/promises"
import { gzipSync } from "node:zlib"
import test from "node:test"
import { CompressedSaveRepository, CompressedLocalStorageSaveRepository, gzipSaveCodec } from "./save-repository.ts"
import { inspectValidatedArchiveLease } from "./validated-archive-lease.ts"
import { readCapturedArchiveChunks } from "./archive-source.ts"
import { currentSaveFixture, withSettledDay } from "./current-save-fixture.ts"
import { createUtf8Sha256Chain } from "./sha256-chain.ts"

const fixture = () => withSettledDay(currentSaveFixture(), "2030-01-01", "2030-01-02")
const encoded = (value: unknown) => `gzip:${gzipSync(JSON.stringify(value)).toString("base64")}`

test("租约入口仅getItem一次，后续验证/摘要/重放绑定该次捕获值", { timeout: 10000 }, async () => {
  const value = fixture(), text = JSON.stringify(value), original = encoded(value), replacement = encoded({ ...value, seed: "43" })
  let reads = 0, stored = original
  const repository = new CompressedSaveRepository({ getItem(key) { assert.equal(key, "quick"); reads++; const result = stored; stored = replacement; return result }, setItem: async () => { throw new Error("读取不得写入") } }, "quick")
  const lease = await repository.loadValidatedLease()
  assert.ok(lease)
  assert.equal(reads, 1)
  const details = inspectValidatedArchiveLease(lease)
  assert.equal(details.seed, "42"); assert.equal(details.productionAdmission, false)
  assert.equal(details.utf8Bytes, Buffer.byteLength(text))
  const replay: string[] = [], hash = await createUtf8Sha256Chain()
  for await (const chunk of readCapturedArchiveChunks(details.source)) { replay.push(chunk); await hash.update(chunk) }
  assert.equal(replay.join(""), text)
  assert.deepEqual(await hash.finish(), details.contentDigest)
  assert.equal(reads, 1, "重放不得再次读取快速槽")
})

test("空槽返回null，LocalStorage适配器继承显式入口但不自动调用", { timeout: 10000 }, async () => {
  let reads = 0
  const empty = new CompressedSaveRepository({ getItem: async () => { reads++; return null }, setItem: async () => false })
  assert.equal(await empty.loadValidatedLease(), null); assert.equal(reads, 1)
  const repository = new CompressedLocalStorageSaveRepository({ getItem: () => { reads++; return encoded(fixture()) }, setItem: () => { throw new Error("不得写入") } })
  assert.equal(reads, 1, "构造repository不得自动读取或签发")
  assert.ok(await repository.loadValidatedLease()); assert.equal(reads, 2)
})

test("预先取消不读槽，pending storage取消及时结束并观察晚到拒绝", { timeout: 10000 }, async () => {
  let reads = 0
  const controller = new AbortController(), reason = new Error("明确取消")
  controller.abort(reason)
  const untouched = new CompressedSaveRepository({ getItem: () => { reads++; return null }, setItem: async () => false })
  await assert.rejects(untouched.loadValidatedLease(controller.signal), error => error === reason); assert.equal(reads, 0)
  let rejectRead: (error: Error) => void = () => { throw new Error("测试未读取") }
  const waiting = new CompressedSaveRepository({ getItem: () => { reads++; return new Promise((_resolve, reject) => { rejectRead = reject }) }, setItem: async () => false })
  const active = new AbortController(), pending = waiting.loadValidatedLease(active.signal)
  active.abort(reason)
  await assert.rejects(pending, error => error === reason)
  const unhandled: unknown[] = [], listener = (error: unknown) => { unhandled.push(error) }
  process.on("unhandledRejection", listener)
  try { rejectRead(new Error("取消后存储晚到失败")); await setImmediate(); assert.deepEqual(unhandled, []) }
  finally { process.removeListener("unhandledRejection", listener) }
  assert.equal(reads, 1)
})

test("解压期间取消保留原reason，不签发lease或继续读槽", { timeout: 10000 }, async () => {
  let reads = 0
  const repository = new CompressedSaveRepository({ getItem: () => { reads++; return encoded(fixture()) }, setItem: async () => false })
  const controller = new AbortController(), reason = new Error("解压期间取消"), original = ReadableStreamDefaultReader.prototype.read
  ReadableStreamDefaultReader.prototype.read = async function () { const result = await original.call(this); if (!result.done && typeof result.value === "string") controller.abort(reason); return result }
  try { await assert.rejects(repository.loadValidatedLease(controller.signal), error => error === reason) }
  finally { ReadableStreamDefaultReader.prototype.read = original }
  assert.equal(reads, 1)
})

test("坏前缀/gzip/JSON/schema和未日结显错，不fallback或重读", { timeout: 10000 }, async () => {
  for (const [raw, pattern] of [["{}", /gzip:/], ["gzip:%%%", /解压浏览器存档失败：/], [`gzip:${gzipSync('{"seed":1,"x":1,"x":2}').toString("base64")}`, /重复 JSON 键/], [encoded({ ...fixture(), seed: false }), /seed/], [encoded(currentSaveFixture()), /日终/]] as const) {
    let reads = 0
    const repository = new CompressedSaveRepository({ getItem: () => { reads++; return raw }, setItem: async () => false })
    await assert.rejects(repository.loadValidatedLease(), pattern); assert.equal(reads, 1)
  }
})

test("storage同步/异步失败保持读取包装和原cause", { timeout: 10000 }, async () => {
  const failure = new Error("存储故障")
  for (const getItem of [() => { throw failure }, async () => { throw failure }]) {
    const repository = new CompressedSaveRepository({ getItem, setItem: async () => false })
    await assert.rejects(repository.loadValidatedLease(), error => error instanceof Error && /读取浏览器存档失败.*存储故障/.test(error.message) && error.cause === failure)
  }
})

test("自定义codec和mock显式拒绝，不能静默使用默认decoder签发", { timeout: 10000 }, async () => {
  let reads = 0, decodes = 0
  const custom = { ...gzipSaveCodec, async decode() { decodes++; return JSON.stringify(fixture()) } }
  const repository = new CompressedSaveRepository({ getItem: () => { reads++; return "gzip:bad" }, setItem: async () => false }, "quick", custom)
  await assert.rejects(repository.loadValidatedLease(), /固定.*gzip|自定义.*codec/)
  assert.equal(reads, 0); assert.equal(decodes, 0)
})
