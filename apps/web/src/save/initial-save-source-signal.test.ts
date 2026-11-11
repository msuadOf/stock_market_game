import assert from "node:assert/strict"
import test from "node:test"
import { InitialSaveSource } from "./session-replacement.ts"

test("冻结选择期间拒绝更换，过期 signal 不得取得冻结权", { timeout: 10000 }, async () => {
  const source = new InitialSaveSource<number>(); source.select(7);
  const signal = source.selectionSignal(), release = source.freezeSelection(signal);
  assert.throws(() => source.select(9), /重载交接/);
  assert.throws(() => source.reset(), /重载交接/);
  assert.equal(await source.read(async () => 0), 7);
  release(); source.select(9);
  assert.throws(() => source.freezeSelection(signal), /选择已更换/);
  assert.equal(await source.read(async () => 0), 9);
});

test("已开始提交的选择不能被重载冻结抢占", { timeout: 10000 }, () => {
  const source = new InitialSaveSource<number>(), signal = source.selectionSignal();
  const finish = source.beginSelectionChange();
  assert.throws(() => source.freezeSelection(signal), /选择正在提交/);
  source.select(7); finish();
  const release = source.freezeSelection(source.selectionSignal()); release();
});

test("首次读取signal属于选择而非effect，多次read共享且select/reset才取消", { timeout: 10000 }, async () => {
  const source = new InitialSaveSource<number>()
  let signal: AbortSignal | undefined, reads = 0
  let resolve: (value: number) => void = () => { throw new Error("未开始读取") }
  const first = source.read(async selectionSignal => { signal = selectionSignal; reads++; return new Promise(done => { resolve = done }) })
  const second = source.read(async () => { throw new Error("不得重复读") })
  assert.equal(first, second)
  await Promise.resolve()
  assert.ok(signal instanceof AbortSignal)
  assert.equal(signal.aborted, false)
  source.select(9)
  assert.equal(signal.aborted, true)
  resolve(7); assert.equal(await first, 7, "不消费signal的旧读取契约保持，调用方仍须忽略过期结果")
  assert.equal(await source.read(async () => { throw new Error("明确选择不得读槽") }), 9)
  assert.equal(reads, 1)
  source.reset(); assert.equal(await source.read(async () => { throw new Error("明确新局不得读槽") }), null)
})

test("换选择会中止支持取消的pending读取，不中止新选择", { timeout: 10000 }, async () => {
  const source = new InitialSaveSource<number>()
  let started: () => void = () => { throw new Error("未设置进入回执") }
  const entered = new Promise<void>(resolve => { started = resolve })
  let observedSignal: AbortSignal | undefined
  const old = source.read(signal => {
    observedSignal = signal; started()
    assert.ok(signal instanceof AbortSignal)
    return new Promise((_resolve, reject) => { signal.addEventListener("abort", () => { reject(signal.reason) }, { once: true }) })
  })
  const observed = old.catch(error => error)
  await entered
  assert.ok(observedSignal instanceof AbortSignal)
  source.select(2)
  assert.ok(await observed instanceof Error)
  assert.equal(await source.read(async () => { throw new Error("不重读") }), 2)
})


test("共享读取释放屏障等待所有旧读取，清理故障跨reset/select保持且不覆盖新值", { timeout: 10000 }, async () => {
  const source = new InitialSaveSource<number>()
  let finishFirst!: () => void, failSecond!: (error: unknown) => void
  const first = source.runSelectionRead(() => new Promise<void>(resolve => { finishFirst = resolve }))
  const second = source.runSelectionRead(() => new Promise<void>((_resolve, reject) => { failSecond = reject }))
  const observedSecond = second.catch(error => error)
  await Promise.resolve()
  source.reset()
  let settled = false
  const barrier = source.waitReadRelease().then(() => { settled = true; return undefined }, error => { settled = true; return error })
  finishFirst(); await first; await Promise.resolve()
  assert.equal(settled, false, "仍有第二个reader持有资源")
  const failure = new AggregateError([new Error("releaseLock失败")], "reader清理失败")
  failSecond(failure)
  assert.equal(await observedSecond, failure)
  assert.equal(await barrier, failure)
  source.reset(); source.select(3)
  assert.equal(await source.read(async () => { throw new Error("不得重读") }), 3)
  await assert.rejects(source.waitReadRelease(), error => error === failure)
  assert.equal(source.readReleaseFailure(), failure)
})

test("普通schema拒绝和纯选择取消不锁释放屏障，多个清理故障均保留", { timeout: 10000 }, async () => {
  const source = new InitialSaveSource<number>(), schemaFailure = new Error("schema非法")
  await assert.rejects(source.runSelectionRead(async () => { throw schemaFailure }), error => error === schemaFailure)
  await source.waitReadRelease()
  const signal = source.selectionSignal()
  const cancel = source.runSelectionRead(async () => { throw signal.reason })
  source.reset()
  await assert.rejects(cancel, error => error === signal.reason)
  await source.waitReadRelease()
  const first = new AggregateError([new Error("cancel失败")], "第一项清理失败")
  const second = new AggregateError([new Error("unlock失败")], "第二项清理失败")
  await assert.rejects(source.runSelectionRead(async () => { throw first }), error => error === first)
  await assert.rejects(source.runSelectionRead(async () => { throw second }), error => error === second)
  const failure = source.readReleaseFailure()
  assert.ok(failure instanceof AggregateError)
  assert.deepEqual(failure.errors, [first, second])
  await assert.rejects(source.waitReadRelease(), error => error === failure)
})
