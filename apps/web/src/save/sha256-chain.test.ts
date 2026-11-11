import assert from "node:assert/strict"
import { createHash } from "node:crypto"
import test from "node:test"
import { addSha256ChainByteLength, createSha256Chain, SHA256_CHAIN_ALGORITHM, SHA256_CHAIN_BLOCK_BYTES, SHA256_CHAIN_MAX_BYTES } from "./sha256-chain.ts"

// 此参考使用 Node crypto 与明确编码的独立帧；结果不是标准 SHA-256(file)。
function reference(bytes: Uint8Array): string {
  const domain = Buffer.from("stock-market-game/archive-digest/sha256-chain-v1\0", "ascii")
  let state = createHash("sha256").update(domain).update(Uint8Array.of(0)).digest()
  let blocks = 0n
  for (let offset = 0; offset < bytes.length; offset += 65536) {
    const block = bytes.subarray(offset, offset + 65536)
    const sizes = Buffer.alloc(12); sizes.writeBigUInt64BE(blocks); sizes.writeUInt32BE(block.length, 8)
    state = createHash("sha256").update(domain).update(Uint8Array.of(1)).update(state).update(sizes).update(block).digest()
    blocks++
  }
  const final = Buffer.alloc(16); final.writeBigUInt64BE(blocks); final.writeBigUInt64BE(BigInt(bytes.length), 8)
  return createHash("sha256").update(domain).update(Uint8Array.of(2)).update(state).update(final).digest("hex")
}
async function actual(bytes: Uint8Array, widths: readonly number[]) {
  const hasher = await createSha256Chain()
  let offset = 0, turn = 0
  while (offset < bytes.length) {
    const size = Math.min(widths[turn++ % widths.length]!, bytes.length - offset)
    await hasher.update(bytes.subarray(offset, offset + size)); offset += size
  }
  return hasher.finish()
}
function bytes(length: number): Uint8Array { return Uint8Array.from({ length }, (_, index) => (Math.imul(index, 73) + (index >>> 7)) & 255) }

test("空流、ASCII和canonical块边界与独立Node参考帧一致", { timeout: 10000 }, async () => {
  for (const value of [new Uint8Array(), new TextEncoder().encode("abc"), ...[1, 55, 56, 63, 64, 65535, 65536, 65537, 131072, 131073].map(bytes)]) {
    const result = await actual(value, [65536])
    assert.equal(result.digest, reference(value))
    assert.equal(result.algorithm, "sha256-chain-v1")
    assert.equal(result.blockBytes, 65536)
    assert.equal(result.byteLength, BigInt(value.length))
    assert.equal(result.blocks, BigInt(Math.ceil(value.length / 65536)))
    assert.ok(Object.isFrozen(result))
  }
})

test("输入切法不改变canonical块、空update不产生额外块", { timeout: 10000 }, async () => {
  const value = bytes(131089), expected = reference(value)
  for (const widths of [[1], [7, 65536, 53], [65535, 2, 8191], [64, 1024]]) assert.equal((await actual(value, widths)).digest, expected)
  const hasher = await createSha256Chain(); await hasher.update(new Uint8Array()); await hasher.update(value.subarray(0, 1)); await hasher.update(new Uint8Array())
  assert.equal((await hasher.finish()).digest, reference(value.subarray(0, 1)))
})

test("块序、最后一块长度和总长都绑定摘要，不等于标准整文件SHA-256", { timeout: 10000 }, async () => {
  const a = new Uint8Array(65536).fill(65), b = new Uint8Array(65536).fill(66)
  const ab = await createSha256Chain(); await ab.update(a); await ab.update(b)
  const ba = await createSha256Chain(); await ba.update(b); await ba.update(a)
  assert.notEqual((await ab.finish()).digest, (await ba.finish()).digest)
  assert.notEqual((await actual(a, [65536])).digest, (await actual(a.subarray(0, 65535), [65536])).digest)
  assert.notEqual((await actual(new TextEncoder().encode("abc"), [3])).digest, createHash("sha256").update("abc").digest("hex"))
  assert.equal(SHA256_CHAIN_ALGORITHM, "sha256-chain-v1"); assert.equal(SHA256_CHAIN_BLOCK_BYTES, 65536)
})

test("长度使用u64精确累加，拒绝溢出、负值及不安全number", { timeout: 10000 }, () => {
  assert.equal(SHA256_CHAIN_MAX_BYTES, 18446744073709551615n)
  assert.equal(addSha256ChainByteLength(9007199254740993n, 65536), 9007199254806529n)
  assert.equal(addSha256ChainByteLength(SHA256_CHAIN_MAX_BYTES - 1n, 1), SHA256_CHAIN_MAX_BYTES)
  for (const [current, extra] of [[-1n, 0], [SHA256_CHAIN_MAX_BYTES + 1n, 0], [SHA256_CHAIN_MAX_BYTES, 1], [0n, -1], [0n, 0.5], [0n, Infinity], [0n, Number.MAX_SAFE_INTEGER + 1]] as const) assert.throws(() => addSha256ChainByteLength(current, extra))
})

test("finish后及并发操作拒绝，非法输入不破坏已有状态", { timeout: 10000 }, async () => {
  const hasher = await createSha256Chain()
  await assert.rejects(hasher.update(new Uint8Array(65537)), /单次|分块|上限/)
  await assert.rejects(hasher.update("abc" as unknown as Uint8Array), /Uint8Array/)
  await hasher.update(Uint8Array.of(65))
  assert.equal(hasher.byteLength, 1n)
  const result = await hasher.finish(); assert.equal(result.digest, reference(Uint8Array.of(65)))
  await assert.rejects(hasher.finish(), /结束|完成/); await assert.rejects(hasher.update(new Uint8Array()), /结束|完成/)
  const other = await createSha256Chain(), input = bytes(65536), expected = reference(input)
  const pending = other.update(input)
  input.fill(0)
  await assert.rejects(other.update(Uint8Array.of(1)), /进行中|并发/)
  await assert.rejects(other.finish(), /进行中|并发/)
  await pending
  assert.equal((await other.finish()).digest, expected, "update在await前取得有界输入副本，不受caller后续修改影响")
})

test("重复使用固定小输入可处理多MB流，不保留caller各块", { timeout: 10000 }, async () => {
  const input = bytes(65536), hasher = await createSha256Chain()
  for (let index = 0; index < 128; index++) await hasher.update(input)
  const result = await hasher.finish()
  assert.equal(result.byteLength, 8388608n); assert.equal(result.blocks, 128n)
  const check = await createSha256Chain()
  for (let index = 0; index < 128; index++) { await check.update(input.subarray(0, 1)); await check.update(input.subarray(1)) }
  assert.equal((await check.finish()).digest, result.digest)
})

test("固定Node参考向量锁定算法域和序列化格式", { timeout: 10000 }, async () => {
  for (const [text, digest] of [
    ["", "02b13154936b780eac55b72feaad784fa47ac8929fac8713dbad0d250710ddc7"],
    ["abc", "73749ff0d3aec37581459936c211f2f02c2b40799bedc1ba294223470cd8cef8"],
    ["中文🚀", "9dcc433430b46c32f5cf72383b7fa4136bc67166f7f762d7ad4a138c7581756f"],
  ]) assert.equal((await actual(new TextEncoder().encode(text), [2, 3])).digest, digest)
})

test("拒绝SharedArrayBuffer与Proxy输入，不保留之后被改写的caller缓冲", { timeout: 10000 }, async () => {
  const hasher = await createSha256Chain()
  await assert.rejects(hasher.update(new Uint8Array(new SharedArrayBuffer(1))), /SharedArrayBuffer/)
  await assert.rejects(hasher.update(new Proxy(Uint8Array.of(1), {})), /Uint8Array/)
  const input = Uint8Array.of(1, 2, 3), pending = hasher.update(input)
  input.fill(255); await pending
  assert.equal((await hasher.finish()).digest, reference(Uint8Array.of(1, 2, 3)))
})
