import assert from "node:assert/strict"
import test from "node:test"
import { createSha256Chain, createUtf8Sha256Chain } from "./sha256-chain.ts"

async function byteDigest(text: string) {
  const bytes = new TextEncoder().encode(text), hasher = await createSha256Chain()
  for (let offset = 0; offset < bytes.length; offset += 65536) await hasher.update(bytes.subarray(offset, offset + 65536))
  return hasher.finish()
}

test("UTF-8跨每个surrogate边界、空块及未配对替换与整体TextEncoder一致", { timeout: 10000 }, async () => {
  for (const text of ["", "abc", "中文🚀😀尾", "\ud800", "\udc00", "\ud800x", "\ud800\ud800\udc00", "a\udc00\ud800b😀"]) {
    const expected = await byteDigest(text)
    for (let split = 0; split <= text.length; split++) {
      const hasher = await createUtf8Sha256Chain()
      await hasher.update(text.slice(0, split)); await hasher.update(""); await hasher.update(text.slice(split))
      assert.deepEqual(await hasher.finish(), expected)
    }
  }
})

test("UTF-8大文本按固定内部片段处理，字符分块和byte分块摘要相同", { timeout: 10000 }, async () => {
  const text = "中文🚀a".repeat(18000), expected = await byteDigest(text)
  for (const width of [1, 16383, 16384, 65536]) {
    const hasher = await createUtf8Sha256Chain()
    for (let offset = 0; offset < text.length; offset += width) await hasher.update(text.slice(offset, offset + width))
    assert.deepEqual(await hasher.finish(), expected)
  }
})

test("UTF-8状态机拒绝非字符串、finish后和并发调用", { timeout: 10000 }, async () => {
  const hasher = await createUtf8Sha256Chain()
  await assert.rejects(hasher.update(null as unknown as string), /字符串/)
  await hasher.update("\ud800")
  const result = await hasher.finish(); assert.deepEqual(result, await byteDigest("\ud800"))
  await assert.rejects(hasher.update(""), /结束|完成/); await assert.rejects(hasher.finish(), /结束|完成/)
  const other = await createUtf8Sha256Chain(), pending = other.update("x".repeat(65536))
  await assert.rejects(other.update("a"), /进行中|并发/); await assert.rejects(other.finish(), /进行中|并发/)
  await pending
  assert.deepEqual(await other.finish(), await byteDigest("x".repeat(65536)))
})
