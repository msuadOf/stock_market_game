# 周末披露失败复核记录

日期：2026-10-30

## 结论

`weekend_report_publishes_without_trade` 的失败来自测试过度要求整个 `npc_attention` 保存值完全不变。项目 Q11 明确规定非交易自然日仍可执行个人信息检查；`docs/simulation-calendar.md` §4 也规定休市日日终先处理到期事项。故休市日 `next_information_check` 可按账户 cadence 合法推进，不是产品行为错误。

## 源码与失败证据

- `packages/engine/src/session/notices.rs`：`NpcInformationCadence::next_after` 定义 `Immediate`、每日1/2次、自然日间隔及自然月推进方式；`record_information_check` 将下一检查时点写入 `next_information_check`。
- `agents/remaining-questions-and-features/q11-notice-cadence.md`：登记非交易自然日可进行信息检查，以及低频散户日终执行到期检查。
- 原失败记录：`agents/branch-completion/validation-scheduler-2026-10-29/integration-unreached-e1af5aaa-2026-10-30/failed-cases.json`，`weekend_publish::weekend_report_publishes_without_trade`。红日志显示受影响账户的 `next_information_check` 改变；相应 `information_cadence`、`base_probability`、`next_attention_candidate_tick`、`rng_state` 保持一致。`GameSession::save` 汇总 RNG 也在原测试中保持一致。

## 修改与验证

仅调整 `packages/engine/tests/publications/weekend_publish.rs`：新增根据保存前 cadence、旧检查时点与周六披露时点独立推算预期值的测试辅助函数；逐账户核对账户集合、cadence、预期检查时点、基础注意力概率、候选 tick 和 RNG 状态。是否推进检查时点依据保存前 `belief_books` 的参与者键集独立推导；该键集恢复为 `belief_participants`，也是 runtime `INFO` 名册的资格源。未参与信息投递的账户要求整个 attention 行保持不变。没有按实际结果回填预期，也没有删除注意力断言。`git diff --check` 通过。未运行测试。
