import assert from "node:assert/strict"
import { gzipSync } from "node:zlib"
import test from "node:test"
import { captureGzipArchiveSource, readCapturedArchiveChunks, scanCapturedArchive } from "./archive-source.ts"
import { RawSaveNumber } from "./schema/primitives.ts"

test("捕获源不可变且两遍都读取同一 payload，不冒充 schema 已验证", { timeout: 10000 }, async () => {
  let raw = 'gzip:{"seed":"42","snapshot":{"tick":3,"seq":4}}'
  const source = captureGzipArchiveSource(raw)
  raw = "gzip:另一个源"
  assert.equal(Object.isFrozen(source), true)
  assert.equal(source.schemaValidated, false)
  const seen: string[] = []
  const decoder = { async *decodeChunks(payload: string) { seen.push(payload); yield payload } }
  const first = await scanCapturedArchive(source, decoder)
  const second = await scanCapturedArchive(source, decoder)
  assert.deepEqual(first, second)
  assert.deepEqual(seen, ['{"seed":"42","snapshot":{"tick":3,"seq":4}}', '{"seed":"42","snapshot":{"tick":3,"seq":4}}'])
  assert.equal(first.rawMetadata.seed, "42")
  assert.deepEqual(first.rawMetadata["snapshot.tick"], new RawSaveNumber("3"))
  assert.equal(first.schemaValidated, false)
  assert.throws(() => Object.assign(source, { encodedCharacters: 0 }), TypeError)
})

test("伪造或复制身份不能取回源，格式错误不被静默修复", { timeout: 10000 }, async () => {
  const source = captureGzipArchiveSource("gzip:eA==")
  const forged = structuredClone(source)
  await assert.rejects(async () => { for await (const chunk of readCapturedArchiveChunks(forged)) void chunk }, /捕获的身份/)
  assert.throws(() => captureGzipArchiveSource("plain JSON"), /gzip:/)
  assert.throws(() => captureGzipArchiveSource(null as unknown as string), /字符串/)
})

test("真实 gzip 扫描保留字节计数及解压错误优先级", { timeout: 10000 }, async () => {
  const text = '{"seed":"中文😀","badSchema":true}'
  const encoded = gzipSync(text).toString("base64")
  const scan = await scanCapturedArchive(captureGzipArchiveSource(`gzip:${encoded}`))
  assert.equal(scan.utf16Characters, text.length)
  assert.equal(scan.utf8Bytes, Buffer.byteLength(text))
  assert.equal(scan.rawMetadata.seed, "中文😀")
  const duplicate = gzipSync('{"x":1,"x":2}').toString("base64")
  await assert.rejects(scanCapturedArchive(captureGzipArchiveSource(`gzip:${duplicate}`)), /重复 JSON 键/)
  await assert.rejects(scanCapturedArchive(captureGzipArchiveSource(`gzip:${duplicate.slice(0, -8)}`)), /解压浏览器存档失败：/)
})

test("明确传入的坏 decoder 不得被默认 codec 掩盖", { timeout: 10000 }, async () => {
  const source = captureGzipArchiveSource(`gzip:${gzipSync("{}").toString("base64")}`)
  await assert.rejects(scanCapturedArchive(source, {} as never), /缺少分块解码能力/)
})
