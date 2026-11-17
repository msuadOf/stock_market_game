// 完整产品验证合同；只读component轨迹不能进入此证据入口。
import assert from "node:assert/strict";
import { AUTHORITY_SCENARIO, validateAuthorityTrace } from "./session-authority-trace.mjs";
export const PRODUCT_EXTENSIONS = Object.freeze([
  "prepare-rejected-running-preserved", "shutdown-known-archive-failure-cleanup",
  "shutdown-unknown-worker-exit-no-reuse", "terminal-queue-full-prefix-or-gap",
  "stale-release-same-generation", "restore-postcommit-io", "shutdown-pending",
  "multi-observer-stalled", "disconnect-resync-stale",
]);
function notApplicable(value, host, id) {
  const allowed = (id === "shutdown-unknown-worker-exit-no-reuse" && ["native", "linux-tauri"].includes(host))
    || (id === "multi-observer-stalled" && ["linux-tauri", "chromium-shared-wasm"].includes(host));
  const item = value.notApplicable?.[id];
  return allowed && item && typeof item.mechanism === "string" && item.mechanism.length > 0
    && typeof item.reason === "string" && item.reason.length > 0 ? item : null;
}
export function requireProductContractCapabilities(value, host) {
  assert.equal(value?.verification, true, "完整故障合同仅允许隔离验证构建");
  assert.equal(value.scenarioVersion, 1, "未知产品合同版本");
  assert.ok(Array.isArray(value.supportedActions), "未接线：缺少supportedActions");
  for (const id of [...AUTHORITY_SCENARIO.map(row => row.id), ...PRODUCT_EXTENSIONS]) {
    if (notApplicable(value, host, id)) continue;
    assert.ok(value.supportedActions.includes(id), `未接线：${host}/${id}`);
  }
  return value;
}
export function validateProductContractEvidence(value) {
  assert.equal(value.execution, "real", "只接受真实产品运行，不接受component或仅编译证据");
  assert.equal(value.kind, "authority-product-contract/v1");
  validateAuthorityTrace(value);
  assert.ok(value.productTransport && typeof value.productTransport.kind === "string", "缺少实际产品传输证据");
  assert.ok(Array.isArray(value.productTransport.events) && value.productTransport.events.length > 0, "没有真实产品收包证据");
  assert.equal(value.extensions.length, PRODUCT_EXTENSIONS.length, "扩展合同不完整");
  for (const [index, extension] of value.extensions.entries()) {
    assert.equal(extension.id, PRODUCT_EXTENSIONS[index]);
    if (extension.status === "not-applicable") {
      assert.ok(notApplicable({ notApplicable: { [extension.id]: extension } }, value.host, extension.id), "非法N/A或缺少机制说明");
    } else {
      assert.equal(extension.status, "passed", "扩展未通过不能产生成功产品证据");
      assert.ok(extension.evidence && Object.keys(extension.evidence).length > 0, "扩展缺少实际采样");
    }
  }
  return value;
}

/** port由真实浏览器WS或真实Tauri窗口IPC建立；此处不填造任何状态。 */
export async function executeProductContract(port) {
  const capability = requireProductContractCapabilities(await port.capabilities(), port.host);
  const rows = [];
  let primary;
  try {
    await port.create(capability.setup, capability.seed);
    for (const { id } of AUTHORITY_SCENARIO) {
      await port.observeBefore(id);
      await port.action(id);
      await port.observeAfter(id);
      rows.push(await port.snapshot(id));
    }
  } catch (error) { primary = error; throw error; }
  finally { try { await port.close(); } catch (error) { throw primary ? new AggregateError([primary, error], "产品合同失败并且清理失败") : error; } }
  const extensions = [];
  // 每项破坏性边界使用独立真实产品会话，不能复用已terminal/shutdown owner。
  for (const id of PRODUCT_EXTENSIONS) {
    const absent = notApplicable(capability, port.host, id);
    if (absent) { extensions.push({ id, status: "not-applicable", ...absent }); continue; }
    const isolated = await port.fork();
    let primary, evidence;
    try {
      await isolated.create(capability.setup, capability.seed);
      evidence = await isolated.extension(id);
      assert.ok(evidence && Object.keys(evidence).length > 0, `${id}没有真实边界证据`);
      extensions.push({ id, status: "passed", evidence });
    } catch (error) { primary = error; throw error; }
    finally { try { await isolated.close(); if (evidence) evidence.cleanupTransport = await (isolated.cleanupEvidence?.() ?? isolated.transportEvidence()); } catch (error) { throw primary ? new AggregateError([primary, error], "产品扩展失败并且清理失败") : error; } }
  }
  return { rows, extensions, productTransport: port.transportEvidence() };
}
