# Continuous、事实消费与 P0 独立复核

- 复核者：`review_continuous_core`，未参与本批产品代码或测试实施。
- 基线：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。
- 日期：2026-10-03。
- 范围：父任务指定的 A05、N01、R2N01、N04、N06、N09、N10；以下 17 个文件的完整 diff、当前生产源码及测试源码均已阅读。补充检查了 `GameSession` 状态定义、NPC lifecycle 注册/移除 caller 和共同 DayEnd clear caller。
- 验证方式：只读静态复核。按父任务限制，没有运行 cargo、测试、构建或新增 subagent；本记录不宣称编译或测试通过。

## 结论

发现 1 处状态封装后的 caller 遗漏，实施者已修复并经本复核者再次核对。当前指定范围没有未关闭的有效发现；没有发现交易语义回归、扩大领域范围或新的不必要抽象。此结论只覆盖指定 diff 的静态独立门禁，不代替父任务的运行验证和全批集成验收。

## 有效发现与复核关闭

`mod.rs:252–253` 的 `plan_tick` closure 曾保留 `game.last_retail_decisions.clear()` 和 `game.last_retail_order_events.clear()`。`GameSession` 当前仅拥有 poison/故障注入元数据与 `state`；两个 Vec 已迁入 `CommittableSessionState`，旧字段访问会阻断编译。

已向实施者报告精确位置。最终完整 diff 改为 `game.state.last_retail_decisions.clear()`、`game.state.last_retail_order_events.clear()`；复核确认清理位置、P0/P1 顺序和原数据保持一致，发现关闭。仅通过源码确认修复，不把此确认写成编译成功。

实施者另修复了 `decision_resources_tests.rs` 的四处 private `Position` fixture 写口；已重新阅读最终完整 diff。`Position::from_restored_parts` 仅替换原本需要修改的 `qty` 或 `t1_locked`，保留另一数量字段与 invested/recovered facts；原断言没有删除或弱化。

## 大 A 语义和依据

依据为 ADR-0017 顶部 2026-09-24/25 修订、其现行阶段/资源/失败契约，ADR-0018 §7，以及 `docs/trading-rules.md` 登记的官方规则和游戏简化。后者记录沪深 2026 版规则 2026-07-06 施行、2026-09-25 价格时间优先条款复核；中国结算费用表 404 与沿用 2026-09-08 依据的限制仍明确保留。本批没有修改交易制度、板块建模、费率、T+1 或市价类型，也没有声称新查验官方来源。

- 同股顺序：`ContinuousStockRoundProcessor::run` 按 supplied operations 逐项处理；coordinator 按股票分组只保留本股输入顺序。股票结果排序仅发生在处理后，用于合并和稳定选首错，不以 candidate、sealed index 或 OrderId 建立交易优先级。既有测试覆盖下降身份和逆序输入。
- consuming shadow：`IncrementalContinuousStockShadow::apply_round` / `finish` 只迁移原归属状态；worker 失败仍使 coordinator `failed = true`，后续 round/finish 明确拒绝，已消费股票状态不提供局部重试。原完整 tick candidate 丢弃边界保持。
- 收据：`ContinuousFillReceiptProjection` 保持 resolution 后 incoming ordinal 从 1 起、maker 从 0 起；每笔 trade 先买方后卖方，逐 envelope audit 和 ordinal 续接；`FillTransition` 的名义/实收费用、卖方 fee cap、cash/shares 分量和 market remainder release 原样保留。无 P4 账户结算或 P1 回补。
- 事实消费：`PlanChainFactConsumption::prepare_*` 保持每个 fact 的四集合身份检查后立即做 payload 校验，再检查 receipts 的原交错首错顺序；增量集合只在全部私有 market/parent/plan 投影与同步成功后 `commit_round`。finalizer 通过只读 contains 查询核对，不引入第二次 Settlement。
- 生命周期诊断：`ContinuousLifecycleEventBatch` 仅聚合同订单临时依赖。数量链构造、Submitted 防重、缓冲 terminal 与 unresolved 首错顺序保持；成功后才一次 append Session，不把跨订单输出排序当作交易时间。
- DayEnd：closing price/depth 在清簿前冻结，全部 continuation 排空后 finish 一次；Settlement 使用原 tick moment，随后共同 DayEnd 执行 T+1 unlock。`ContinuousDayEndLifecycleProjection` 保留 index 检查前原有的 parent/NPC/retail candidate 写入，typed 失败由外层丢弃整个 candidate，不擅自增加局部原子性承诺。
- NPC lifecycle：`ensure_order_absent` caller 位于原 phase/account/parent 排除之后、placed minute/lifetime/day cap 计算之前，保持全局 OrderId duplicate panic 时点。remove 使用原 account/code/order 三字段谓词，due 迭代顺序及外层 expiry 排序不变。只为 Resting 普通 NPC 注册，即时全成不注册，同 round 后续全成去掉暂存项。
- P0/P1：`ExpiryOutput::record_release` 先 checked_add 账户资源再 append 明细；P0 仍只在 Continuous 执行，并通过 post-P0 live 量影响一次 P1 seal。`DecisionResourceSnapshot` 不另加 aggregate releases；卖单 cash 预留恒为零。

## 必要性、范围和测试边界

新增 owner 都对应 assigned-actions 中原本共同推进的状态，不引入 trait、长期 manager、第二套 ledger、DTO/存档变化或新增依赖。`quote_expiry` 可见性仅扩大到 `crate::session`，用于原 Session lifecycle caller；ExpiryOutput 原 public 构造兼容性保持。

新增短 fixture 覆盖 symbolic resolution + 两 maker ordinal、consuming finish 清簿前 depth/单 release、事实 preparation 可丢弃及混合错误先后、诊断数量/Submitted/terminal/unresolved 错误、DayEnd index 溢出前原部分 candidate 写入、lifecycle 三字段删除和全局 duplicate、release 账户累计溢出不 append。

既有源码覆盖跨轮买卖 maker 部分成交/撤余量、两个 worker 首错扰动与 stopped、事实恰一次消费、缺 acceptance quote 不登记 consumed、当轮释放不回补、P0 buy/sell 释放只计一次、finalizer 日终 T+1 顺序、后尾失败整 tick 回滚与 save/restore。未发现本次迁移新增的未覆盖语义分支；测试源码证据不是运行结果。

## 复核文件清单与内容绑定

哈希清单由最终文件内容生成，位于下面的代码块；SHA-256 组合摘要按文件路径排序，以 `路径 + NUL + 文件 SHA-256 + 换行` 连接后计算。范围外后续改动不自动获得本次复核结论，范围内内容变化须再次复核。

```text
9904016208096d0c20a3565fc13ee06937e946d67d6d8a02064f1a3e7add5826  packages/engine/src/session/pipeline/adaptive_plan_chain.rs
8632c86c50676203c91151700f956127d0dc346db081b867c8a5ffa28ce5835e  packages/engine/src/session/pipeline/adaptive_plan_chain_tests.rs
4e46884d6b2bf8eb6dad91886c518a1719afe4ec1e70b494033afe4b91828a58  packages/engine/src/session/pipeline/continuous_lifecycle_projection.rs
7d833ea793bca35078a1c38c5f88746285f90701aa5c312b233e70aa215bab89  packages/engine/src/session/pipeline/continuous_matching.rs
df52a6920761ab758bb725a94b05ae40e20ed4028aa689bd9a33f261ebe6763b  packages/engine/src/session/pipeline/continuous_matching_tests.rs
96829d0a386463e276abcd5938d3d3ac0df99176ff9cdd554c27ee45762ed19e  packages/engine/src/session/pipeline/continuous_tick_finalizer.rs
a5028f9f38e784ee7ed604c07c3fe15d601c401b9c91dccb8abc9c9a73e2572e  packages/engine/src/session/pipeline/continuous_tick_finalizer_tests.rs
f738eeb1ab65de56770faa97dae9a0669b829dfc6b8dc26c652499c6afe232da  packages/engine/src/session/pipeline/decision_resources.rs
90e15b1e9a308f6fedfd7263136bf7e805c2020e1d0512a09a34354db39d30ef  packages/engine/src/session/pipeline/decision_resources_tests.rs
8007ec010def75a3c71d20f93b648bf481684121ffbeb881c40f8016f29c28b4  packages/engine/src/session/pipeline/incremental_continuous_stock_shadow.rs
6f0fe31143372eb7741d1ed6ae75550eb23ea127910a92fc9ee03e505f7fce8c  packages/engine/src/session/pipeline/incremental_continuous_stock_shadow_tests.rs
da6fb7835858000f792287dd0ffb284a5062e89df0946b4903112a1bf69817cc  packages/engine/src/session/pipeline/initial_candidate_round_tests.rs
c72782c4bf4b4699922311da8038467fd0a4b531c9051106fd3b1b3493243d97  packages/engine/src/session/pipeline/mod.rs
0753be5b857ab86a06c65101f3f5e4073b84de8f0da3bb9ad0d824f7d0178e89  packages/engine/src/session/pipeline/npc_lifecycle_projection_tests.rs
8ccf1e5a49b369f4864cedc0bdff8d3917682b84662ad7fe0e0e5bb18cf5fe78  packages/engine/src/session/pipeline/quote_expiry.rs
370de23def02457d49bff6cfd0fd7b3d9d57c1d6ac732b8b6499324a059c807f  packages/engine/src/session/pipeline/quote_expiry_checkpoint_tests.rs
68dcef530d65dbcf3879ce5b25db19b7953664cb448a686aff6a4b6fb0c97a62  packages/engine/src/session/pipeline/quote_expiry_tests.rs
```

组合 SHA-256：`ecc516acb97a2547c7f115075b444846d138d01d9470a542b0e783646dbe3546`。

## 编译 04 后 fixture 收口再次复核

已重新全文阅读 `adaptive_plan_chain_tests.rs` 相对上述 baseline 的完整 diff。最终第 325、404 行分别使用 `Position::from_restored_parts(50, 0, 0, 0)` 与 `(100, 0, 0, 0)`，constructor 四参数按 qty/t1_locked/invested_cents/recovered_cents 原样赋值，保持原零股 fixture、T+1 与资金成本事实；没有更改断言或其他行为。

将两处 constructor 文本还原为上次已审阅的 Position literal 后，文件 SHA-256 与上一绑定完全一致；其余 16 个文件 SHA-256 均未变化。因此此次增量仅为两个 private 字段构造替换，未发现新的有效问题。未运行 cargo 或测试。上面的 17 文件清单与组合哈希已更新为本次最终内容。

历史绑定（由本次绑定替代）：组合 `8bb9309732071c736fa506eb8311f34d6578fb6446cef416456661235dff0581`；adaptive 测试文件 `eef619f273044ff7e7b6525cf38e1de8bb0f7ef484fbb887064c6761fd025c87`。

## 最终证据 manifest

本独立复核者再次逐文件比较上述已审阅绑定与当前内容：17/17 一致，无漂移。已生成 [review-continuous-core-manifest.json](review-continuous-core-manifest.json)，逐文件关联最终 SHA-256 与本记录，并分别登记完整 baseline diff、编译 02 的四处 Position fixture 收口和编译 04 的两处 literal 收口的独立复核证据。本次仅更新工作记录和 manifest，没有修改产品源码或测试，也没有运行 cargo 或测试。
