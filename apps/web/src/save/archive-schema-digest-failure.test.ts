import assert from "node:assert/strict"
import { gzipSync } from "node:zlib"
import test from "node:test"
import { captureGzipArchiveSource } from "./archive-source.ts"
import { currentSaveFixture, withSettledDay } from "./current-save-fixture.ts"

// 文件隔离的原生故障注入；正式入口与摘要组件均不接受调用者的 digest/decoder。
const subtle = globalThis.crypto.subtle, original = subtle.digest, descriptor = Object.getOwnPropertyDescriptor(subtle, "digest")
let calls = 0, failAt = 1
const failure = new Error("原生摘要服务失败")
Object.defineProperty(subtle, "digest", { configurable: true, writable: true, value: function (algorithm: AlgorithmIdentifier, data: BufferSource) {
  if (++calls === failAt) return Promise.reject(failure)
  return original.call(subtle, algorithm, data)
} })
let validation: typeof import("./archive-schema-validation.ts"), leases: typeof import("./validated-archive-lease.ts")
try { validation = await import("./archive-schema-validation.ts"); leases = await import("./validated-archive-lease.ts") }
finally {
  if (descriptor === undefined) delete (subtle as Partial<SubtleCrypto>).digest
  else Object.defineProperty(subtle, "digest", descriptor)
}
function captured(text: string) { return captureGzipArchiveSource(`gzip:${gzipSync(text).toString("base64")}`) }
function reset(at: number) { calls = 0; failAt = at }

test("原生摘要初始化/尾块故障不能生成报告或租约", { timeout: 10000 }, async () => {
  const source = captured(JSON.stringify(withSettledDay(currentSaveFixture(), "2030-01-01", "2030-01-02")))
  for (const at of [1, 2, 3]) {
    reset(at)
    await assert.rejects(validation.validateCapturedArchiveSchema(source), error => error instanceof Error && /内容摘要失败/.test(error.message) && error.cause === failure)
    reset(at)
    await assert.rejects(leases.createValidatedArchiveLease(source), error => error instanceof Error && /内容摘要失败/.test(error.message) && error.cause === failure)
  }
})

test("锁存摘要故障后仍排空gzip，后到gzip和词法错误保持优先", { timeout: 10000 }, async () => {
  const text = '{"seed":false,"x":1,"x":2,"tail":"' + "x".repeat(100000) + '"}'
  const compressed = gzipSync(text), broken = captureGzipArchiveSource(`gzip:${compressed.subarray(0, -8).toString("base64")}`)
  for (const at of [1, 2]) {
    reset(at)
    await assert.rejects(leases.createValidatedArchiveLease(captured(text)), /包含重复 JSON 键/)
    reset(at)
    await assert.rejects(leases.createValidatedArchiveLease(broken), /解压浏览器存档失败：/)
  }
})

test("摘要故障不能盖过随后明确取消的原始reason", { timeout: 10000 }, async () => {
  reset(1)
  const controller = new AbortController(), reason = new Error("计算摘要期间取消"), originalRead = ReadableStreamDefaultReader.prototype.read
  ReadableStreamDefaultReader.prototype.read = async function () { const result = await originalRead.call(this); if (!result.done && typeof result.value === "string") controller.abort(reason); return result }
  try { await assert.rejects(leases.createValidatedArchiveLease(captured('{"x":1,"x":2}'), controller.signal), error => error === reason) }
  finally { ReadableStreamDefaultReader.prototype.read = originalRead }
})
