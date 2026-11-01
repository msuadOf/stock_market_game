// 对实际 WASM 公共入口验证三档策略；当前缺口应表现为显式拒绝。
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
const pkg = process.argv[2];
assert.ok(pkg, '必须提供本次构建的 WASM 包路径');
const wasm = await import(`${pkg}/web_wasm.js`);
wasm.initSync({ module: await readFile(`${pkg}/web_wasm_bg.wasm`) });
const setup = {
  stocks: [{ code: '600101', exchange: 'Shanghai', initial_price: '1120', category: 'MainBoard', limit_pct: 0.1, tick: '1', total_shares: '8928571429', float_shares: 169318418 }],
  npcs: { retail_count: 1, inst_count: 0, hot_count: 0, retail_cash_median: '20000000' },
  config: { commission_rate: 0.00025, commission_min: '500', stamp_tax_rate: 0.0005, default_limit: 0.1, st_limit: 0.1, price_cage_enabled: true, lot_size: 100, starting_cash: '1000000000' },
  strategy_params: { retail: { arrival_rate: 0.3, order_size_mean: 300, chase_prob: 0.4 }, inst: { margin: 0.02, order_size: 200000 }, hot: { lookback: 20, trend_threshold: 0.03, order_size: 100000 } },
  ticks_per_day: 4, auction_ticks: 1, closing_auction_ticks: 0, history_len: 1, t1_enabled: true,
  float_allocation: { between_kinds: { Percentage: { retail: 1, inst: 0, hot: 0 } }, within_kind: 'Random' },
  start_date: '2030-01-01', simulation_policy_id: 'a-share-simulation', runtime_sync_policy: 'every_tick',
};
for (const policy of ['every_tick', 'day_end', 'on_demand']) {
  try {
    const handle = wasm.create_session({ ...setup, runtime_sync_policy: policy }, 42n);
    console.log(`${policy}: 创建成功 handle=${handle}`);
    wasm.drop_session(handle);
  } catch (error) {
    console.error(`${policy}: 创建失败: ${String(error)}`);
    process.exitCode = 1;
  }
}
