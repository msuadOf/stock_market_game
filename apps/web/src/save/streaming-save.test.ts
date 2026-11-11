import assert from "node:assert/strict"
import { gzipSync } from "node:zlib"
import test from "node:test"
import { CompressedLocalStorageSaveRepository } from "./save-repository.ts"
import { isFrozenJsonArchive, parseFrozenSaveJsonChunks, parseSaveJson } from "./save-schema.ts"
import { currentSaveFixture } from "./current-save-fixture.ts"

async function* split(text: string, width: number): AsyncIterable<string> {
  for (let offset = 0; offset < text.length; offset += width) yield text.slice(offset, offset + width)
}

test("流式冻结入口保持完整存档、数值与不可伪造品牌", { timeout: 10000 }, async () => {
  const text = JSON.stringify(currentSaveFixture())
  for (const width of [1, 7, 16384]) {
    const parsed = await parseFrozenSaveJsonChunks(split(text, width))
    assert.deepEqual(parsed, parseSaveJson(text))
    assert.equal(isFrozenJsonArchive(parsed), true)
    assert.equal(Object.isFrozen(parsed.snapshot.accounts), true)
  }
})

test("WASM 快速槽使用分块 codec 而普通读取保持原接口", { timeout: 10000 }, async () => {
  const text = JSON.stringify(currentSaveFixture())
  let ordinaryReads = 0, chunkReads = 0
  const repository = new CompressedLocalStorageSaveRepository({ getItem: () => "gzip:payload", setItem: () => undefined }, "save", {
    encode: async value => value,
    decode: async () => { ordinaryReads++; return text },
    decodeChunks: async function* () { chunkReads++; yield* split(text, 11) },
  })
  assert.deepEqual(await repository.loadFrozen(), parseSaveJson(text))
  assert.equal(ordinaryReads, 0, "WASM 不应再解压为完整大字符串")
  assert.equal(chunkReads, 1)
  assert.deepEqual(await repository.load(), parseSaveJson(text))
  assert.equal(ordinaryReads, 1)
})

test("晚词法首错优先于早 schema 错误且失败后可新建独立解析", { timeout: 10000 }, async () => {
  const text = '{"badSchema":1,"later":1,"later":2}'
  await assert.rejects(parseFrozenSaveJsonChunks(split(text, 1)), /重复 JSON 键/)
  await assert.rejects(parseFrozenSaveJsonChunks(split('{"badSchema":1}', 1)), /必填/)
  await assert.rejects(parseFrozenSaveJsonChunks(split('"\\ud800"', 1)), /孤立 Unicode surrogate/)
  assert.deepEqual(await parseFrozenSaveJsonChunks(split(JSON.stringify(currentSaveFixture()), 31)), currentSaveFixture())
})

test("锁存词法错误后排空原流，后到 gzip 错误仍优先", { timeout: 10000 }, async () => {
  let drained = false
  const cause = new Error("gzip CRC 损坏")
  const repository = new CompressedLocalStorageSaveRepository({ getItem: () => "gzip:payload", setItem: () => undefined }, "save", {
    encode: async value => value,
    decode: async () => { throw new Error("不应调用整串解码") },
    decodeChunks: async function* () {
      yield '{"x":1,"x"'
      drained = true
      throw cause
    },
  })
  await assert.rejects(repository.loadFrozen(), /解压浏览器存档失败：gzip CRC 损坏/)
  assert.equal(drained, true)
})

test("真实 gzip 的流式坏数据保留解压前缀，合法 gzip 的词法错不冒称解压失败", { timeout: 10000 }, async () => {
  const validGzip = gzipSync('{"x":1,"x":2}').toString("base64")
  let stored = `gzip:${validGzip}`
  const repository = new CompressedLocalStorageSaveRepository({ getItem: () => stored, setItem: () => undefined })
  await assert.rejects(repository.loadFrozen(), error => error instanceof Error && /重复 JSON 键/.test(error.message) && !/解压浏览器存档失败/.test(error.message))
  stored = `gzip:${validGzip.slice(0, -8)}`
  await assert.rejects(repository.loadFrozen(), /解压浏览器存档失败：/)
})

test("流式 codec 保留 UTF-8 BOM 和坏字节替换语义", { timeout: 10000 }, async () => {
  const { gzipSaveCodec } = await import("./save-repository.ts")
  assert.ok(gzipSaveCodec.decodeChunks)
  for (const bytes of [Buffer.from([0xef, 0xbb, 0xbf, 0x61]), Buffer.from([0x61, 0xf0, 0x9f, 0x62])]) {
    const compressed = gzipSync(bytes).toString("base64")
    const parts: string[] = []
    for await (const chunk of gzipSaveCodec.decodeChunks(compressed)) parts.push(chunk)
    assert.equal(parts.join(""), await gzipSaveCodec.decode(compressed))
  }
})

test("词法首错后成功排空不再建图，也不被后续无效 chunk 替换", { timeout: 10000 }, async () => {
  let drained = false
  const source = async function* (): AsyncIterable<string> {
    yield '{"x":1,"x"'
    yield '{"bad":'
    drained = true
  }
  await assert.rejects(parseFrozenSaveJsonChunks(source()), /根节点.x.*重复 JSON 键/)
  assert.equal(drained, true)
})
