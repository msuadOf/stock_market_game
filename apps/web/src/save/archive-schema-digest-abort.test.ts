import assert from "node:assert/strict"
import { setImmediate } from "node:timers/promises"
import { gzipSync } from "node:zlib"
import test from "node:test"
import { captureGzipArchiveSource } from "./archive-source.ts"
import { currentSaveFixture, withSettledDay } from "./current-save-fixture.ts"

const subtle = globalThis.crypto.subtle, original = subtle.digest, descriptor = Object.getOwnPropertyDescriptor(subtle, "digest")
let calls = 0, heldAt = 0
let entered: () => void = () => { throw new Error("测试未设置 digest 进入回执") }
let release: (failure?: Error) => void = () => { throw new Error("测试尚无 pending digest") }
Object.defineProperty(subtle, "digest", { configurable: true, writable: true, value: function (algorithm: AlgorithmIdentifier, data: BufferSource) {
  if (++calls !== heldAt) return original.call(subtle, algorithm, data)
  // 按原生契约先复制输入；仅延迟完成，不将 caller 的可变 view 留到之后。
  const copy = new Uint8Array(data instanceof ArrayBuffer ? data.slice(0) : data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength))
  entered()
  return new Promise<ArrayBuffer>((resolve, reject) => { release = failure => { if (failure !== undefined) reject(failure); else void original.call(subtle, algorithm, copy).then(resolve, reject) } })
} })
let leases: typeof import("./validated-archive-lease.ts")
try { leases = await import("./validated-archive-lease.ts") }
finally {
  if (descriptor === undefined) delete (subtle as Partial<SubtleCrypto>).digest
  else Object.defineProperty(subtle, "digest", descriptor)
}
const fixture = () => withSettledDay(currentSaveFixture(), "2030-01-01", "2030-01-02")

for (const [point, at, large, lateFailure] of [["init", 1, false, true], ["update", 2, true, false], ["finish", 3, false, true]] as const) {
  test(`原生${point} digest pending时取消及时拒绝，不等待原生完成且观察晚到结果`, { timeout: 10000 }, async () => {
    calls = 0; heldAt = at
    const value = fixture()
    if (large) {
      const members = (value.market_memberships as { members: Record<string, unknown> }).members
      members["x".repeat(100000)] = members["local-owner"]; delete members["local-owner"]
    }
    const source = captureGzipArchiveSource(`gzip:${gzipSync(JSON.stringify(value)).toString("base64")}`)
    const controller = new AbortController(), reason = new Error(`等待${point}时取消`), late = new Error("晚到原生故障")
    const unhandled: unknown[] = [], recordUnhandled = (error: unknown) => { unhandled.push(error) }
    process.on("unhandledRejection", recordUnhandled)
    let settled = false
    const started = new Promise<void>(resolve => { entered = resolve })
    const pending = leases.createValidatedArchiveLease(source, controller.signal).then(value => { settled = true; return { ok: true as const, value } }, error => { settled = true; return { ok: false as const, error } })
    let timely = false
    try {
      await started; controller.abort(reason); await setImmediate(); timely = settled
    } finally { release(lateFailure ? late : undefined) }
    const result = await pending
    await setImmediate(); await setImmediate()
    process.removeListener("unhandledRejection", recordUnhandled)
    assert.equal(timely, true, "取消必须在原生 digest 仍 pending 时结束，不签租约")
    assert.equal(result.ok, false)
    if (!result.ok) assert.equal(result.error, reason)
    assert.deepEqual(unhandled, [], "原生摘要晚到 reject 必须已被观察")
  })
}

function largeSource() {
  const value = fixture(), members = (value.market_memberships as { members: Record<string, unknown> }).members
  members["y".repeat(100000)] = members["local-owner"]; delete members["local-owner"]
  return captureGzipArchiveSource(`gzip:${gzipSync(JSON.stringify(value)).toString("base64")}`)
}

test("digest取消仍等待reader取消完成，原生迟到失败被观察", { timeout: 10000 }, async () => {
  calls = 0; heldAt = 2
  const controller = new AbortController(), reason = new Error("摘要取消"), originalCancel = ReadableStreamDefaultReader.prototype.cancel
  let releaseCleanup: () => void = () => { throw new Error("测试未开始清理") }
  let cleanupEntered: () => void = () => { throw new Error("测试未等待清理") }
  const cleanupStarted = new Promise<void>(resolve => { cleanupEntered = resolve })
  ReadableStreamDefaultReader.prototype.cancel = function (cancelReason) {
    cleanupEntered()
    return new Promise<void>((resolve, reject) => { releaseCleanup = () => { void originalCancel.call(this, cancelReason).then(resolve, reject) } })
  }
  const started = new Promise<void>(resolve => { entered = resolve })
  let settled = false
  const pending = leases.createValidatedArchiveLease(largeSource(), controller.signal).then(() => { settled = true; return undefined }, error => { settled = true; return error })
  try {
    await started; controller.abort(reason); await cleanupStarted; await setImmediate()
    assert.equal(settled, false, "reader cancel 未完成时不得谎报已结束")
    releaseCleanup()
    assert.equal(await pending, reason)
  } finally {
    releaseCleanup(); release(new Error("取消后原生迟到故障")); ReadableStreamDefaultReader.prototype.cancel = originalCancel
  }
  await setImmediate()
})

test("digest等待期间取消且reader清理失败仍保留取消reason与清理原因", { timeout: 10000 }, async () => {
  calls = 0; heldAt = 2
  const controller = new AbortController(), reason = new Error("摘要取消"), cleanup = new Error("取消reader失败"), originalCancel = ReadableStreamDefaultReader.prototype.cancel
  ReadableStreamDefaultReader.prototype.cancel = async function () { throw cleanup }
  const started = new Promise<void>(resolve => { entered = resolve })
  const pending = leases.createValidatedArchiveLease(largeSource(), controller.signal).then(() => undefined, error => error)
  let error: unknown
  try { await started; controller.abort(reason); error = await pending }
  finally { release(new Error("原生迟到失败")); ReadableStreamDefaultReader.prototype.cancel = originalCancel }
  assert.ok(error instanceof AggregateError, error instanceof Error ? `${error.name}: ${error.message}; cause=${String(error.cause)}` : String(error))
  assert.ok(error.errors.includes(reason), "必须保留原取消原因")
  assert.ok(error.errors.includes(cleanup), "必须保留reader取消失败")
  await setImmediate()
})
