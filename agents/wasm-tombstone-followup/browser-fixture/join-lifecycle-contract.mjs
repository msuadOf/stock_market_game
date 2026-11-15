import assert from "node:assert/strict";

const JOIN_DONE = 2, JOIN_CONSUMED = 5;
const terminalTypes = new Set(["childExit", "childrenDrained", "finalRelease", "releaseUnknown"]);
const terminalEvents = (events) => events.filter(({ type }) => terminalTypes.has(type));

function assertJoinLifecycleContract(result, mode) {
  assert.equal(result.mode, mode);
  const { identity, beforeJoin, afterJoin, joined, gate, events, counts, failures } = result;
  assert.equal(identity.runtimeName, "verification-owned-worker-handle");
  assert.equal(beforeJoin.join.gen, identity.gen);
  assert.equal(afterJoin.join.gen, identity.gen);
  assert.equal(beforeJoin.join.state, JOIN_DONE);
  assert.equal(afterJoin.join.state, JOIN_CONSUMED);
  assert.deepEqual(beforeJoin.gate, [0, 1, 0, 0], "已发布 DONE 但尚未进入真实 destroy");
  assert.deepEqual(afterJoin.gate, [0, 1, 0, 0], "已消费结果仍停在真正的 destroy 前");
  const input = mode === "normal" ? 0xA701 : mode === "cleanup-failure" ? 0xA702 : 0xA703;
  assert.equal(joined.value, input ^ 0x5A5A, "必须保留本 case 真实 WorkerHandle 子闭包的结果");
  assert.match(joined.duplicate, /无在途句柄/);
  assert.equal(beforeJoin.counts.children, 1);
  assert.equal(afterJoin.counts.children, 1);
  assert.equal(afterJoin.counts.protocolEntries, 1);
  assert.equal(afterJoin.drainState, "pending");
  assert.deepEqual(terminalEvents(afterJoin.events), []);
  assert.equal(result.engineTerminations, 1);
  assert.equal(counts.children, 0);
  assert.equal(counts.workers, 0);
  assert.equal(counts.ports, 0);
  assert.equal(counts.protocolEntries, 0);
  assert.equal(counts.pendingRecovery, 0);
  assert.equal(counts.timers, 0);
  assert.deepEqual(result.anomalies, []);

  if (mode === "normal") {
    assert.deepEqual(gate, [1, 1, 1, 1], "真实 raw destroy 已精确调用并成功返回一次");
    assert.equal(result.drainState, "drained");
    assert.equal(result.childTerminations, 0, "正常 child 自行退出，不能靠 owner terminate 伪造");
    const exits = events.filter(({ type }) => type === "childExit");
    assert.equal(exits.length, 1);
    assert.deepEqual([exits[0].workerId, exits[0].slot, exits[0].gen], [identity.workerId, identity.slot, identity.gen]);
    assert.deepEqual(events.filter(({ type }) => type === "childrenDrained"), [
      { type: "childrenDrained", activeChildren: 0, manifestEntries: 0, pendingRecovery: 0 },
    ]);
    assert.ok(events.some(({ type }) => type === "quiescent"), "engine 也必须提交真实夹具静止证明");
    assert.ok(Number.isSafeInteger(result.sampledBytes) && result.sampledBytes > 0);
    assert.deepEqual(result.receipt, { kind: "released", finalBytes: result.sampledBytes, growthStopped: true, referencesDropped: true });
    assert.equal(events.at(-1)?.type, "finalRelease");
    assert.deepEqual(events.filter(({ type }) => type === "finalRelease"), [
      { type: "finalRelease", kind: "released", finalBytes: result.sampledBytes },
    ]);
    assert.equal(events.some(({ type }) => type === "releaseUnknown"), false);
    assert.deepEqual(failures, []);
  } else {
    assert.equal(result.childTerminations, 1);
    assert.equal(result.drainState, "rejected");
    assert.equal(result.receipt.kind, "unknown");
    assert.match(result.receipt.message, /真实 child exit|停止 grow/);
    assert.deepEqual(terminalEvents(events).map(({ type }) => type), ["releaseUnknown"]);
    if (mode === "forced") {
      // 不是以“没收到 exit”断言 TLS/stack 已处理；这里只证明实际 terminate 且没有正常销毁证明。
      assert.deepEqual(gate, [0, 1, 0, 0], "被实际 terminate 的 child 始终未释放测试闸门");
      assert.deepEqual(failures.map(({ code }) => code), ["JOIN_LIFECYCLE_FIXTURE_FORCED_STOP"]);
    } else {
      assert.deepEqual(gate, [2, 1, 0, 0], "在已进入的 cleanup 真实控制路径抛错；不伪造 raw 已执行");
      assert.deepEqual(failures.map(({ code, where }) => [code, where]), [
        ["FACADE_BOOTSTRAP_FAILURE", "facade-child-bootstrap.cleanup"],
      ]);
      assert.match(failures[0].message, /JOIN_LIFECYCLE_CLEANUP_FAILURE_PROBE/);
    }
  }
}

export async function runJoinLifecycleContract(browser, baseUrl) {
  const cases = await Promise.allSettled(["normal", "cleanup-failure", "forced"].map(async (mode) => {
    const page = await browser.newPage();
    try {
      await page.goto(new URL(`agents/wasm-tombstone-followup/browser-fixture/join-lifecycle.html?mode=${mode}`, baseUrl).href);
      try {
        await page.waitForFunction(() => window.joinLifecycle?.result !== undefined || window.joinLifecycle?.error !== undefined, null, { timeout: 8000 });
      } catch (error) {
        throw new Error(`join 生命周期等待超时：${JSON.stringify(await page.evaluate(() => window.joinLifecycle))}`, { cause: error });
      }
      const evidence = await page.evaluate(() => window.joinLifecycle);
      assert.equal(evidence.error, undefined);
      assert.ok(evidence.result);
      console.log(JSON.stringify({ joinLifecycle: evidence.result }));
      assertJoinLifecycleContract(evidence.result, mode);
    } finally {
      await page.close();
    }
  }));
  const failed = cases.filter(({ status }) => status === "rejected").map(({ reason }) => reason);
  if (failed.length > 0) throw new AggregateError(failed, "真实 Browser join 生命周期验收失败");
}
