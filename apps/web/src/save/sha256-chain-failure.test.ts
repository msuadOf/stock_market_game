import assert from "node:assert/strict"
import test from "node:test"

// 隔离模块实例只替换原生环境故障；组件公开 API 不接受调用者的 digest 参数。
async function failingModule(failAt: number) {
  const subtle = globalThis.crypto.subtle, original = subtle.digest, descriptor = Object.getOwnPropertyDescriptor(subtle, "digest")
  let calls = 0
  const failure = new Error(`原生 SHA-256 故障 ${failAt}`)
  Object.defineProperty(subtle, "digest", { configurable: true, writable: true, value: function (algorithm: AlgorithmIdentifier, data: BufferSource) {
    if (++calls === failAt) return Promise.reject(failure)
    return original.call(subtle, algorithm, data)
  } })
  try {
    const module = await import(`${new URL("./sha256-chain.ts", import.meta.url).href}?failure=${failAt}`) as typeof import("./sha256-chain.ts")
    return { module, failure }
  } finally {
    if (descriptor === undefined) delete (subtle as Partial<SubtleCrypto>).digest
    else Object.defineProperty(subtle, "digest", descriptor)
  }
}

test("原生digest初始化失败显式拒绝，不伪造默认hash", { timeout: 10000 }, async () => {
  const { module, failure } = await failingModule(1)
  await assert.rejects(module.createSha256Chain(), error => error === failure)
})

test("update故障与finish故障锁住实例并保留原始cause", { timeout: 10000 }, async () => {
  const duringUpdate = await failingModule(2), hasher = await duringUpdate.module.createSha256Chain()
  await assert.rejects(hasher.update(new Uint8Array(65536)), error => error === duringUpdate.failure)
  await assert.rejects(hasher.update(Uint8Array.of(1)), error => error instanceof Error && error.cause === duringUpdate.failure)
  await assert.rejects(hasher.finish(), error => error instanceof Error && error.cause === duringUpdate.failure)
  const duringFinish = await failingModule(3), partial = await duringFinish.module.createSha256Chain()
  await partial.update(Uint8Array.of(65))
  await assert.rejects(partial.finish(), error => error === duringFinish.failure)
  await assert.rejects(partial.finish(), error => error instanceof Error && error.cause === duringFinish.failure)
})

test("UTF-8内层故障后不能继续或再次finish", { timeout: 10000 }, async () => {
  const { module, failure } = await failingModule(4), hasher = await module.createUtf8Sha256Chain()
  await hasher.update("a".repeat(65536))
  await hasher.update("x")
  await assert.rejects(hasher.finish(), error => error === failure)
  await assert.rejects(hasher.update("b"), error => error instanceof Error && error.cause === failure)
  await assert.rejects(hasher.finish(), error => error instanceof Error && error.cause === failure)
})
