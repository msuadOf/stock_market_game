import assert from "node:assert/strict";

/** 同一命令与故障脚本；真实端口必须实现全部操作，不能用空操作替代。 */
export const AUTHORITY_SCENARIO = Object.freeze([
  { id: "initial", operation: "create-paused" },
  { id: "manual", operation: "step-once" },
  { id: "without-observer", operation: "advance-day-with-observer-detached" },
  { id: "slow-observer", operation: "advance-day-with-ack-withheld" },
  { id: "archive-full", operation: "inject-archive-queue-full" },
  { id: "blocked", operation: "attempt-continue-after-archive-failure" },
  { id: "old-receipt", operation: "deliver-old-writer-receipt" },
  { id: "bad-receipt", operation: "deliver-out-of-order-receipt" },
  { id: "duplicate-receipt", operation: "deliver-duplicate-receipt" },
  { id: "prepare-rejected", operation: "inject-replacement-prepare-failure" },
  { id: "replaced", operation: "replace-from-sealed-checkpoint" },
  { id: "old-generation", operation: "deliver-old-generation-packet" },
  { id: "new-lease", operation: "reattach-observer-in-same-generation" },
  { id: "old-lease", operation: "deliver-old-lease-ack-release-cut" },
  { id: "prefix-failure", operation: "inject-failure-after-committed-prefix" },
  { id: "terminal-repeat", operation: "attempt-repeat-terminal-and-continue" },
  { id: "shutdown", operation: "shutdown-and-confirm-archive-durability" },
].map(Object.freeze));
const HOSTS = ["native", "chromium-shared-wasm", "linux-tauri"];
const decimal = (value, field) => {
  assert.ok(typeof value === "string" && /^(0|[1-9][0-9]*)$/.test(value), `${field} 必须是十进制整数串`);
  return BigInt(value);
};
function checkpoint(value, field) {
  assert.ok(value !== null && typeof value === "object", `${field} 缺少 checkpoint key`);
  decimal(value.generation, `${field}.generation`);
  decimal(value.writerEpoch, `${field}.writerEpoch`);
  return decimal(value.seq, `${field}.seq`);
}

/** 验收格式校验与语义比较不能替代真实端口中的场景断言。 */
export function validateAuthorityTrace(trace) {
  assert.ok(HOSTS.includes(trace.host), "未知 trace 宿主");
  assert.equal(trace.execution, "real", "trace 必须来自实际运行；仅编译结果不合格");
  return validateContractFacts(trace);
}

function validateContractFacts(trace) {
  assert.equal(trace.schema, 1, "未知 authority trace schema");
  for (const field of ["sourceSha256", "artifactSha256"]) {
    assert.match(trace[field], /^[0-9a-f]{64}$/, `${field} 缺少实际指纹`);
  }
  assert.deepEqual(trace.scenario, AUTHORITY_SCENARIO.map(({ id }) => id), "trace 缺少统一场景");
  assert.equal(trace.rows.length, AUTHORITY_SCENARIO.length, "trace 场景样本不完整");
  for (const [index, row] of trace.rows.entries()) {
    assert.equal(row.id, AUTHORITY_SCENARIO[index].id, "trace 样本顺序错误");
    const generation = decimal(row.generation, "generation");
    assert.ok(generation > 0n && Number.isSafeInteger(row.day) && row.day >= 0, "无效 generation/day");
    decimal(row.tick, "tick");
    const seq = decimal(row.seq, "seq");
    assert.equal(typeof row.paused, "boolean", "缺少暂停事实");
    assert.ok(row.terminal === null || typeof row.terminal === "string", "缺少 terminal 事实");
    assert.equal(typeof row.archive.blocked, "boolean", "缺少 archive blocked 事实");
    assert.ok(Array.isArray(row.archive.queued), "缺少 queued 事实");
    for (const key of row.archive.queued) checkpoint(key, "queued");
    if (row.archive.durable !== null) {
      assert.equal(row.archive.durable.generation, row.generation, "durable 属于旧 generation");
      assert.ok(checkpoint(row.archive.durable, "durable") <= seq, "durable 不得超过已提交 seq");
    }
    const observer = row.observer;
    decimal(observer.generation, "observer.generation");
    assert.ok(decimal(observer.lease, "observer.lease") > 0n, "observer lease 必须为正");
    const sent = decimal(observer.sent, "observer.sent");
    const flushed = decimal(observer.flushed, "observer.flushed");
    const accepted = decimal(observer.accepted, "observer.accepted");
    assert.ok(flushed <= sent, "flushed 不得超过 sent");
    assert.ok(accepted <= sent, "accepted 不得超过 sent；应用接纳不能冒充传输发送");
    // 两种 ACK 不强制同一物理时机，不能要求 accepted === flushed。
  }
  const installed = trace.rows.find(row => row.id === "new-lease").observer;
  const afterStale = trace.rows.find(row => row.id === "old-lease").observer;
  assert.equal(afterStale.generation, installed.generation, "旧lease动作改变了generation");
  assert.equal(afterStale.lease, installed.lease, "旧lease动作改变了当前lease");
  assert.equal(afterStale.accepted, installed.accepted, "旧lease ACK推进了应用游标");
  return trace;
}

/** 端口由真实 Native/Chromium/Tauri 驱动器提供；这里不创建模拟结果。 */
export async function runAuthorityScenario(port) {
  const rows = [];
  for (const command of AUTHORITY_SCENARIO) {
    await port.execute(command);
    const row = await port.capture(command.id);
    assert.ok(row !== undefined, `真实端口没有捕获 ${command.id}`);
    rows.push(structuredClone(row));
  }
  return validateAuthorityTrace({
    schema: 1, host: port.host, execution: "real",
    sourceSha256: port.sourceSha256, artifactSha256: port.artifactSha256,
    scenario: AUTHORITY_SCENARIO.map(({ id }) => id), rows,
  });
}

export function compareAuthorityTraces(traces) {
  assert.equal(traces.length, HOSTS.length, "必须有三端实际运行 trace");
  const seen = new Set();
  for (const trace of traces) {
    validateAuthorityTrace(trace);
    assert.ok(!seen.has(trace.host), "重复宿主 trace");
    seen.add(trace.host);
    assert.equal(trace.sourceSha256, traces[0].sourceSha256, "三端源码指纹不一致");
  }
  compareContractRows(traces);
  return { comparedRows: AUTHORITY_SCENARIO.length, hosts: [...seen], sourceSha256: traces[0].sourceSha256 };
}


function compareContractRows(traces) {
  // 不比较成交账户/资金字节，也不把 socket flush 时机强制等同 Worker 接纳时机。
  // 真实断开/重新订阅可能使用不同数量lease；身份和旧包隔离逐端验证。
  // 不把传输续租计数或baseline接纳游标当作市场语义。
  const key = value => value === null ? null : ({ generation: value.generation, writerEpoch: value.writerEpoch, seq: value.seq });
  // 显式投影共同合同；civilReady及存储/传输诊断仍保留在原始trace中。
  const semantic = trace => trace.rows.map(row => ({
    id: row.id, generation: row.generation, day: row.day, tick: row.tick, seq: row.seq,
    paused: row.paused, terminal: row.terminal,
    archive: { queued: row.archive.queued.map(key), durable: key(row.archive.durable), blocked: row.archive.blocked },
  }));
  for (const trace of traces.slice(1)) {
    assert.deepEqual(semantic(trace), semantic(traces[0]), `${trace.host} authority 语义不一致`);
  }
}

/** 组件实测和真实Browser的合同对照；不能充当三端端到端验收。 */
export function compareAuthorityContractEvidence(traces) {
  const expected = new Map([
    ["native-actor-component", "component"],
    ["tauri-actor-component", "component"],
    ["chromium-shared-wasm", "real"],
  ]);
  assert.equal(traces.length, expected.size, "混合合同必须包含两种actor组件和真实Browser");
  const seen = new Set();
  for (const trace of traces) {
    assert.ok(expected.has(trace.host), "未知合同证据宿主");
    assert.equal(trace.execution, expected.get(trace.host), `${trace.host}证据级别不匹配，不能升级或使用仅编译结果`);
    assert.ok(!seen.has(trace.host), "重复合同证据宿主");
    seen.add(trace.host);
    validateContractFacts(trace);
    assert.equal(trace.sourceSha256, traces[0].sourceSha256, "合同源码指纹不一致");
  }
  compareContractRows(traces);
  return {
    schema: "authority-contract-evidence/v1", level: "mixed-level/non-E2E", endToEnd: false,
    comparedRows: AUTHORITY_SCENARIO.length, sourceSha256: traces[0].sourceSha256,
    exclusions: ["Native组件不代表真实HTTP/WebSocket部署", "Tauri组件不代表Linux窗口实跑"],
    evidence: structuredClone(traces),
  };
}
