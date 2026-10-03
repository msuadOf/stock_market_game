# experience / behavior / indicators 实施记录

- 日期：2026-10-03。
- 独立复核基线：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。
- 指令中的具名动作是 `domain-N01`、`domain-N07`、`domain-R2-N28`、`domain-R2-N34`、`domain-R2-N35`、`domain-R2-N40`，合计 6 个 ID；`domain-N07` 包含观察与持仓对账两个转换。
- 权威正文：[`action-index.md`](../../oop-refactor-audit/challenge-2026-10-03/action-index.md) 对应各动作完整段落。
- 状态：本文件簇 owner 迁移已实施，最终独立静态复审通过，无未关闭有效发现；统一构建与产品测试仍由 root 安排，本文不宣称运行验收完成。

## 前置边界

已阅读根 `AGENTS.md`、`docs/principles.md`、`docs/testing.md`、`docs/architecture.md`、`docs/open-questions.md`，以及 ADR-0006、ADR-0013、ADR-0021、ADR-0026 和所涉完整源码。改动位于 engine 纯逻辑层，未新增依赖、I/O 或持久字段。

策略阈值、经历衰减与关注 cap 为 ADR-0013/0026 明确的游戏假设；指标不是权威成交价。仓位输出继续按 Money 分、股份股、fraction 比例表达，买入目标保留原 100 股增量及已有零股余数，T+1 继续只约束 executable delta。未改变交易制度；现行规则依据沿用 ADR-0021 第 5 节已登记的沪深 2026 规则（2026-07-06 施行）。本轮未重新联网取证，不将此实现记录当成新的交易所规则验证。

## 逐动作结果

| 动作 | 实际 owner 与 caller | 保持的边界 |
|---|---|---|
| domain-N01 | `indicators.rs::KdjAccumulator` 持有 previous K/D 与 Kdj 三序列；`kdj_ohlc`、`kdj_from_values` 直接调用 `push_rsv` / `finish` | 九样本窗口与 high/low fold 留在各原 caller；K→D→J 算式和执行顺序逐项保持，seed 50、空输入无 seed 样本 |
| domain-N07（含 domain-R2-E03 增强） | `RetailExperienceState` 组合私有 `PositionExperienceTransition` 的同股 legacy / 可选 active epoch 借用，六个原 writer 真实使用其 fill / observe / reset / dated observation / fee seed 方法；机构观察与 stale 清理两个公开 State 转换已由 session caller 接入 | threshold bp 由 session 明确传入；价格→三个时钟→双 map 存在守卫顺序保持；legacy 与 active epoch 键集独立；原计数/冷静期 overflow 部分写入保留，clock 与退出/失败历史仍在原成功点提交；费用及散户/机构冷却差异保持 |
| domain-R2-N28 | `RetailPositionDecisionContext` 持有股票、行情/账户风险借用和具名仓位输入；`decision.rs` 全部原目标/信心调用改用 `target_for` / `apply_experience_confidence` | 风险筛选、优先级、风格分支、arrival gate 和 RNG 调用位置不移动；目标整手/零股、desired 与 executable、T1Locked 原规则保持 |
| domain-R2-N34 | `StockPriceMemory` 私有 first observation / observe / public-history-read / time guard；集合公共 API 查找或创建后委托条目 | 非正价格守卫仍先于 map 查找；回拨先于任何写入；公开读不创造本人观察；read-count overflow 前已写 public-read minute、未写 last_touched 的原失败面保持 |
| domain-R2-N35 | 新增私有 `experience/retention.rs::RetentionCandidates`，拥有显式 contact 候选、protected 借用、cap；两个集合分别调用 selection 并自行 retain | watchlist 使用 attention 分钟，price memory 使用 touch 分钟；受保护不计 cap；未保护 cap 8、minute/code 降序稳定破同分；watchlist 列表时钟及恢复边界保持 |
| domain-R2-N40 | `EmaSmoother` 持有 previous 与显式双系数；macd 的 fast、slow、DEA 三实例各自产生原序列 | fast/slow 原 12/26 参数；DEA 明确 `0.2` / `0.8`，不改写为相减；各路 seed/遍历顺序及 DIF→DEA→histogram 顺序保持 |

N07 的 session 文件不在本 agent 写入范围。已确认 session owner 的 `session/institutional_behavior.rs` 真实调用 `observe_institution_position_dated`，`session.rs` stale 清理真实调用 `clear_stale_institutional_holding`。六个内部 writer 为 `record_fill_dated`、`record_institutional_fill_dated`、`initialize_holding_dated`、`initialize_institutional_holding_dated`、`observe_position_dated` 和被委托的 `record_fill_with_order`；它们均接入新增 `experience/position_transition.rs::PositionExperienceTransition`。legacy `observe_position` 同步复用原观察写体，保持 dated 的既有委托。State 保留 map 生命周期与账户 clock/history orchestration，借用对象拥有原 fill/observe 变更体与同股 epoch 观察/费用转换，初始化同时生成匹配的 legacy 与新 epoch 内容，不形成另一个持久 owner。

`PositionExperienceTransition::from_maps` 仅在各原守卫通过后按既有 `entry.or_default` 取得 legacy 行，epoch 为可选借用，不新增存在性验证；清仓后 legacy 历史行继续保留。新增保护明确覆盖 active epoch 存在但 legacy 缺行时的机构 fill 原接受集、清仓行再入场、retail 观察缺 legacy 的原接受集，以及 retail/机构 invalid transition 与 NoActiveEntry 的不同首错。

## 短行为保护与 TDD 事实

先于相应实现写入的保护测试：指标 32 样本 EMA 与 KDJ 第 9/10 样本边界、价格错误/overflow 失败面、watchlist cap/恢复边界、仓位整手/零股/T+1，以及 N07 两个目标 API 的状态转换测试。随后补充信心 RNG 消费、两种淘汰时钟差异、无效行情/权益与缺失风险输入的回归边界。N07 增强实施前再次先写三个现有 API 保护测试，固定 retail counter/cooldown overflow 的 legacy 部分写入与 feedback 不提交、institutional initialize/fee 的守卫顺序和双 map 不同键集；复核补充机构 fill 缺 legacy 的原接受集用例。N07 的 6 个短用例已机械移到 `experience/feedback/lifecycle/institutional_transition_tests.rs`，模块路径/filter 保持。

执行限制明确禁止本 agent 运行 Cargo 或产品测试，因此没有本地断言红/绿证据；新增 N07 API 实施前的缺方法状态只是预期编译红，不冒充有效断言红。基线/绿灯验证请求与 filters 已交给统一 runner。

默认 features 足够，新增短 lib filters：

- `indicators::tests::recurrence_protection_spans_ema_period_and_kdj_window_rollover`
- `experience::price_memory::protection_tests`
- `experience::watchlist::protection_tests`
- `experience::retention::tests`
- `experience::feedback::lifecycle::institutional_transition_tests`
- `behavior::heuristics::position_context_tests`

建议统一 runner 复用已有 integration binaries 的 `behavior`、`experience`、`experience_feedback`、`technical_memory`、`attention_discovery` 以及 session 的 N07 观察/恢复用例。普通测试执行与单 case 上限均为 10000ms；冷编译独立分类，多核并发与外部 deadline 由协调方记录。未要求全量回归或长期统计验收。

本 agent 仅运行所修改文件的 `rustfmt --config skip_children=true` 和 scoped `git diff --check`，均成功；未运行 Cargo、产品测试、Git 写命令或全仓 fmt。未改变 serde/TS 字段及公开 DTO，新增 owner 均不入存档。

## 独立复审与最终短验证

原未实施改动的 `review_experience` 已对最终完整 baseline diff 独立复审，六 writer 组合、Optional epoch、机构费用、legacy 部分写入与首错保持均核销；新增未跟踪的 `position_transition.rs`、测试文件和 `retention.rs` 全文及 SHA 已纳入。最终无未关闭有效发现，记录见 [`experience-review.md`](experience-review.md)。本簇新增短保护共 14 个，均未由本 agent 执行。

root 最终冻结后编译及 14 个指定短保护 case 已通过，包含 N07 双记录增强的四个增量 case。运行产物通过最终 build07/check08 核验；证据见 [final-summary.md](final-summary.md)。本组没有未完成事项，不以短验证宣称完整回归通过。
