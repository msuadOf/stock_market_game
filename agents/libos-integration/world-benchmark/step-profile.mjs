import assert from 'node:assert/strict';

// 这些段包含嵌套关系，不能直接相加当作总耗时。
export const STEP_SEGMENT_NAMES = Object.freeze(['world_run','vwap_calc','acct_echo_collect','acct_writeback','fill_projection','event_assembly','belief_echo','hold_inits','plans_echo','memory_prune','market_install','attention_heap','candle_install','price_tick_emit','retail_diag','envelope','desk_mirror','day_end','baseline_snapshot','step_outer','driver_step','step_entry','lock_route','step_via_body','undo_clear','belief_collect','envelope_collect','world_converge','world_seal','world_install','converge_receive']);
const names = new Set(STEP_SEGMENT_NAMES);

export function validateStepProfile(rows, expectedTicks) {
  assert.ok(Array.isArray(rows), '画像结果必须为数组');
  assert.ok(Number.isSafeInteger(expectedTicks) && expectedTicks > 0, 'tick 数无效');
  const result = new Map();
  for (const row of rows) {
    assert.ok(Array.isArray(row) && row.length === 3, '画像记录形状无效');
    const [name, ns, count] = row;
    assert.ok(names.has(name) && !result.has(name), '画像段名称无效或重复');
    assert.match(ns, /^(0|[1-9][0-9]*)$/, '纳秒必须为非负整数文本');
    assert.ok(Number.isSafeInteger(count) && count >= 0, '画像次数无效');
    result.set(name, {ns:BigInt(ns),count});
  }
  for (const required of STEP_SEGMENT_NAMES) assert.ok(result.has(required), `缺少画像段 ${required}`);
  assert.equal(result.get('world_run').count, expectedTicks, 'world_run 次数不符');
  assert.ok(result.get('world_run').ns > 0n, 'world_run 时长不得为零');
  for (const name of ['world_seal','world_install']) {
    assert.equal(result.get(name).count, expectedTicks, `${name} 次数不符`);
    assert.ok(result.get(name).ns > 0n, `${name} 时长不得为零`);
  }
  assert.ok(result.get('world_converge').count >= expectedTicks && result.get('world_converge').ns > 0n, 'world_converge 次数或时长不符');
  assert.ok(result.get('converge_receive').count >= result.get('world_converge').count && result.get('converge_receive').ns > 0n, 'converge_receive 次数或时长不符');
  return result;
}

export function validateRequestedProfile(result, mode) {
  assert.ok(mode === '0' || mode === '1', '画像模式必须为 0 或 1');
  if (mode === '1') validateStepProfile(result.stepProfile, 3000);
  else assert.ok(result.stepProfile === undefined || result.stepProfile === null, '未请求画像却收到画像账本');
}
