import {test} from 'node:test';
import assert from 'node:assert/strict';
import {continuousAcceleration} from './acceleration.mjs';

const setup = {ticks_per_day:15300, auction_ticks:900, closing_auction_ticks:180};
test('连续交易窗口按照游戏时钟计算模拟秒数和加速比', {timeout:10000}, () => {
  assert.deepEqual(continuousAcceleration(setup, {from:1000,to:4000}, 1250), {simulatedSeconds:3000, acceleration:2400});
});
test('跨阶段或无效的输入必须拒绝', {timeout:10000}, () => {
  for (const [s,w,t] of [
    [setup,{from:899,to:1000},10], [setup,{from:1000,to:15121},10],
    [setup,{from:4000,to:1000},10], [setup,{from:1000,to:4000},0],
    [{...setup,ticks_per_day:900}, {from:1000,to:4000},100],
    [setup,{from:1000.5,to:4000},100],
    [setup,{from:1000,to:4000},Number.MIN_VALUE],
  ]) assert.throws(()=>continuousAcceleration(s,w,t));
});
