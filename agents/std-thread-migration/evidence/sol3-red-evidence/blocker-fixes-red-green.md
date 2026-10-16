# Sol 四审两阻断修复的红绿证据补档（2026-10-15）

## 阻断 1：逐单闭环漏检
- 反例单元测试（flip_helpers closure_tests::sol3r_double_cancel_same_id_is_rejected）
  修复前红（双撤 A 被聚合口径放行）→ 修复后绿。
- 精确闭环测试 batch3_exact_closure_on_no_trade_fixture（seed 302 无成交
  fixture：trades=0/accepts=25/cancels=25 实测）：生产面破坏（投影丢
  首条撤单事件）→ 红："每张受理单恰一条终结撤单：(0,0,1) left:0 right:1"
  → 恢复后绿。
- 措辞：assert_order_lifecycle_closure 已改名 assert_order_event_invariants，
  doc 明确"不构成逐单终结完整性证明"+登记 Sol 静态反例界限
  （Trade 无订单身份，对手腿触碰量可抵消聚合差）——完整性由无成交
  fixture 精确断言承载。

## 阻断 2：K 错误传播测试超时
- 修复前计时（candle-test-slow-before-fix.log）：case 31.41s
  （sever 后 txs 清空、期望响应数不变 → 空等 watchdog 30s）。
- 修复：take_candle_projection/collect_group_attention/snapshot_accounts
  三处通道面一致性守卫（txs 数 ≠ 期望数 → 即时 Disconnected 报错）。
- 修复后：drive_tests 全 3 项 case 合计 1.71s（原单 case 31.4s），
  断言强度保留且增强（错误信息须含"通道面"）。
