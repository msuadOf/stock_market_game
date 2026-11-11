import assert from "node:assert/strict"
import { createHash } from "node:crypto"
import { gzipSync } from "node:zlib"
import test from "node:test"
import { captureGzipArchiveSource } from "./archive-source.ts"
import { validateCapturedArchiveSchema } from "./archive-schema-validation.ts"
import { createValidatedArchiveLease } from "./validated-archive-lease.ts"
import { currentSaveFixture, withSettledDay } from "./current-save-fixture.ts"

const fixture = () => withSettledDay(currentSaveFixture(), "2030-01-01", "2030-01-02")
function reference(text: string) {
  const bytes = Buffer.from(text, "utf8"), domain = Buffer.from("stock-market-game/archive-digest/sha256-chain-v1\0", "ascii")
  let previous = createHash("sha256").update(domain).update(Uint8Array.of(0)).digest(), blocks = 0n
  for (let offset = 0; offset < bytes.length; offset += 65536) {
    const block = bytes.subarray(offset, offset + 65536), header = Buffer.alloc(12)
    header.writeBigUInt64BE(blocks); header.writeUInt32BE(block.length, 8)
    previous = createHash("sha256").update(domain).update(Uint8Array.of(1)).update(previous).update(header).update(block).digest(); blocks++
  }
  const tail = Buffer.alloc(16); tail.writeBigUInt64BE(blocks); tail.writeBigUInt64BE(BigInt(bytes.length), 8)
  return { algorithm: "sha256-chain-v1", blockBytes: 65536, byteLength: BigInt(bytes.length), blocks, digest: createHash("sha256").update(domain).update(Uint8Array.of(2)).update(previous).update(tail).digest("hex") }
}
function source(bytes: Uint8Array | string) { return captureGzipArchiveSource(`gzip:${gzipSync(bytes).toString("base64")}`) }

test("完整验证报告绑定两遍同源UTF-8 descriptor并匹配独立Node参考", { timeout: 10000 }, async () => {
  const value = fixture(), memberships = value.market_memberships as { members: Record<string, unknown> }
  memberships.members["真实中文🚀".repeat(10000)] = memberships.members["local-owner"]
  delete memberships.members["local-owner"]
  const text = JSON.stringify(value), report = await validateCapturedArchiveSchema(source(text))
  assert.deepEqual(Reflect.get(report, "contentDigest"), reference(text))
  assert.equal(report.productionAdmission, false)
  assert.equal(Object.isFrozen(Reflect.get(report, "contentDigest")), true)
})

test("BOM和坏UTF-8仍绑定旧decoder产生的重新编码字节", { timeout: 10000 }, async () => {
  const text = JSON.stringify(fixture())
  const plain = Buffer.from(text), bad = Buffer.from(text)
  bad[bad.indexOf("local-owner")] = 0xff
  for (const bytes of [Buffer.concat([Buffer.from([0xef, 0xbb, 0xbf]), plain]), bad]) {
    const decoded = new TextDecoder().decode(bytes), report = await validateCapturedArchiveSchema(source(bytes))
    assert.deepEqual(Reflect.get(report, "contentDigest"), reference(decoded))
    assert.equal(report.utf8Bytes, Buffer.byteLength(decoded))
  }
})

test("同长度同root键的第二遍内容变化拒绝报告和租约", { timeout: 10000 }, async () => {
  const captured = source(JSON.stringify(fixture())), original = ReadableStreamDefaultReader.prototype.read
  for (const operation of [() => validateCapturedArchiveSchema(captured), () => createValidatedArchiveLease(captured)]) {
    let readers = 0, changes = 0
    const indices = new WeakMap<object, number>()
    ReadableStreamDefaultReader.prototype.read = async function () {
      const result = await original.call(this)
      if (!result.done && typeof result.value === "string") {
        let index = indices.get(this)
        if (index === undefined) { index = ++readers; indices.set(this, index) }
        if (index === 2 && result.value.includes('"seed":"42"')) { changes++; return { done: false, value: result.value.replace('"seed":"42"', '"seed":"43"') } }
      }
      return result
    }
    try { await assert.rejects(operation(), /摘要|内容.*不一致/); assert.equal(changes, 1) }
    finally { ReadableStreamDefaultReader.prototype.read = original }
  }
})

test("摘要wrapper不改变gzip、词法和取消原优先级", { timeout: 10000 }, async () => {
  const text = '{"seed":false,"x":1,"x":2}', payload = gzipSync(text)
  await assert.rejects(validateCapturedArchiveSchema(source(text)), /包含重复 JSON 键/)
  const broken = captureGzipArchiveSource(`gzip:${payload.subarray(0, -8).toString("base64")}`)
  await assert.rejects(validateCapturedArchiveSchema(broken), /解压浏览器存档失败：/)
  const controller = new AbortController(), reason = new Error("摘要读取期间取消")
  const original = ReadableStreamDefaultReader.prototype.read
  ReadableStreamDefaultReader.prototype.read = async function () { const result = await original.call(this); if (!result.done && typeof result.value === "string") controller.abort(reason); return result }
  try { await assert.rejects(createValidatedArchiveLease(source(text), controller.signal), error => error === reason) }
  finally { ReadableStreamDefaultReader.prototype.read = original }
})
