# Session caller 与既有测试补充独立复核

复核者：`/root/implement_session/review_core`，未参与实施。复核日期：2026-10-03。
基线：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。本报告是 [core-review.md](core-review.md) 的独立附件，范围仅为下表追加的 10 个文件；逐项读取了全部基线 diff。

## 结论与三门

当前无阻断静态发现，也未发现新增 counterexample。改动都是原字段访问迁到 `state`、Account getter，或以 test-only fixture setter 保留原非法状态构造；没有新增交易决策、校验或 fallback。

1. **大 A 语义与依据。** 玩家队列仍按全局 FIFO 一次移动，capture 不路由订单、不消耗 OrderId/seq；连续竞价撤单所有权、竞价不能撤单的 tick、symbolic price、原始意图保留，以及 buyer cash/seller shares 的 escrow 语义保持原样。nominal fees、实际 seller charged fees 和路径相关 rounding debt 的断言未变；T+1、分/股单位和日内交易阶段没有变化。依据沿用 core-review 记录的交易规则、ADR-0017 和 ADR-0025，本附件没有新实现交易制度，也未重新联网核验官方资料。
2. **必要性与最小范围。** `player_candidates.rs` 唯一生产改动是 `std::mem::take(&mut self.pending_player)` 改为 `std::mem::take(&mut self.state.pending_player)`；其他追加文件仅迁移既有测试。由于 `GameSession`、Account、Market 的 owner 封装，这些 caller 适配为维持原生产入口和原测试必要，没有扩大公开 API 或引入新依赖。
3. **边界、语义漂移与复杂度。** 逐 hunk 检查后，测试 payload、预期错误类型、hash 比较、ledger 回滚、pure projection、idempotence 与 seq 不变断言保持原样；没有删除、放宽或替换预期值。补充静态计数为 51 个 `#[test]` 与 190 个 assert 宏，基线与现稿一致；计数只作辅助，结论以全文 diff 为依据。`plan_chain_candidates_tests.rs` 为已有 fixture 构造函数，仅给 plans caller 加 `state` 路径。

## 非法 fixture 重点核对

- `failure_tests.rs` 中缺失 NPC strategy 仍由 `AccountBook::get_mut` 后设置 `None`，没有经过重新生成默认策略或输入过滤。`Account::fixture_set_strategy` 只调用原事实写入口 `restore_strategy`，其写入仍是 `Arc::make_mut(...).strategy = strategy`；页校验缓存失效仍由 `AccountBook::get_mut` 执行。
- 注入错误优先于 missing strategy 校验的测试仍保持原入口与预期；poison 后 business hash 不变、session hash 改变，后续 step/save 返回同一个 fatal 的断言未变。
- 私有 plan 失败 case 保留 `strategy = None`、2 股持仓、1 分成本和 `i64::MAX` last_price。新 `Market::fixture_set_last_price` 带 `#[cfg(test)]` 且仅做直接赋值，不做 clamp、校验或价格重建，因此仍能触发原 allocation snapshot overflow；未用正常价格绕开错误。
- `continuous_cancellation_tests.rs` 的缺失 strategy fixture 同样保持 `None`，使本测试继续检查 state-only cancellation，而不是顺带启动 NPC 决策。
- envelope 两组测试继续构造重复 continuous/auction identity、1 分 nominal fee drift、合法 seller charged debt、越界 charge 和缺失 partial seller charged history；发生拒绝后的原 ledger 完整相等断言保持原样。

## 执行与内容绑定

仅做只读源码与 `git diff/show` 检查；未执行 Cargo 或测试，未格式化、未 Git 写入。实施者告知 compile05 已通过，本报告不把该告知冒充复核者亲自执行的验证结果。
下表 10 个 SHA-256 已与 `session/source-manifest.json` 对应条目逐项核对相符。10 文件完整 diff 的 SHA-256：`dc7f70dc4f8e4593747475d8d1498d14e9a37d51d3cc4dfbca54fdec3868f24b`。

| 文件（前缀 packages/engine/src/session/） | SHA-256 |
|---|---|
| hash_contract_tests.rs | 7528667d4c45dc04872597f4d42a63f6a4c02db5fdde2582d06e05398e1d79f5 |
| failure_tests.rs | bcc8dee1db70652464be9be323d596b65f0bb62252102ab251e740c1ff1ea6f0 |
| player_candidates.rs | 2807798b1657b758aaad803f2ca1354e4a5ec9e2994c1f1745c5765cb3d4eb40 |
| player_candidates_tests.rs | 75b468da671ae87e28d3d0b1201923b2f453a406dd6b56e61ea5eff3524398d6 |
| continuous_cancellation_tests.rs | afb784a7151608a274215dd41466383087a3a1ae03f894aa6fe2e51f31a6b7f7 |
| envelope_projection_tests.rs | 340e760df0c974747a839746aeb44ce8b12ef7ce93c7ed455f8d9a807e00b59c |
| envelope_projection_hydration_tests.rs | a336f4761aefb843984dddddee6f311749ad8eeef314d3f0582e6c42ac6744ba |
| reconciliation_plan_phase_tests.rs | b9c4c5276424f929dd31f4f69a9f95eddfc260406a4160cad289983ac389e777 |
| reconciliation_plan_tests.rs | 90ffff1823e22367cefada259dddcbd3566935888f5a1f693fc3932680852cd9 |
| plan_chain_candidates_tests.rs | d6d7d1cf3a94f41df946b2b750d0115f3543f13958e7c375f2ffb39264455c9b |
