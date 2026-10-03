# session leaf 独立复核

日期：2026-10-03。复核者：`/root/implement_session/review_leaf`，未参与实现。baseline：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。

## 范围与方式

完整阅读指定 diff：`plans/**`（排除 `candidates/targets.rs`）、`strategy/**`、`session/institutional_behavior.rs`、`session/company_assembly.rs`、`session/persistence.rs`、`session/persistence/**`、`session/snapshot.rs`。权威需求采用 `session/assigned-actions.json` 与其 action-index 正文；roots 尚在实施的接口缺失不列为本范围发现。

阅读 AGENTS、principles、architecture、open-questions、trading-rules、ADR-0026 与相关 action 正文/实施记录；采用基线与当前源码静态逐项核对。未运行 cargo 或产品测试，未编辑产品源码、执行 Git 写操作、全仓 fmt 或派生 agent。限定范围 `git diff --check` 成功。

## 有效发现

1. **编译阻断：poison 路径迁错。** `session/persistence/v2_tests.rs` 的 `capture_rejects_poisoned_session_without_emitting_a_dto` 把 `session.poison` 改为 `session.state.poison`。ES01-A01 要求 poison 留在 GameSession；当前声明也如此。该处须恢复 facade 字段路径，维持原拒绝 poisoned save 的断言。已通知 parent 与 persistence worker，待修复复核。
2. **编译阻断：TradingPlan caller 漏迁移。** `session/persistence.rs::validate_plan_contract` 仍直接读取 `plan.account/code/target/filled_qty` 和 `linked.account/code`，与 `pub(in crate::plans)` 不兼容。须迁同名 getter，保留原条件、错误文字与顺序。已通知 parent 与 persistence worker，待修复复核。该函数的 ParentOrderPlan 读取不属于同一问题，不应机械替换成 TradingPlan getter。
3. **定向验证缺口：ZiNoiseStrategy 投影未被直接覆盖。** engine-strategy-03-A01 validation 正文要求 helper 引入后定向核对阈值、步幅、观察概率的完整参数投影。当前新增 `strategy_data` 方法没有对应新增断言；既有 `strategy_data_serde_roundtrip` 仅验证 StrategyData 自身。建议用非默认实例逐字段核对私有投影，由 parent 统一运行。已通知 parent 与 plans worker。

## 三门判断

- **大 A 语义与依据：** 静态比较未发现新增交易制度或改变既有交易行为。TradingPlan 的 guard、修改顺序、反向真实成交进度、最后有效交易日日终与既有 VersionOverflow 非原子行为保持；BeliefBook 保留无 peak 的 Option latch、真实 failure 衰减、触发 `>=` 与恢复 `>`，不形成强制卖出或重新抽参数。RetailDecisionContext 保留完整交易分钟观察、主动 Highest/Lowest 意图、T+1 的 sellable_qty、现行零 stop 与无成本比较、噪声卖出重新选股以及 RNG 消费次序。CumulativeFeeAuditV2 保留买方 nominal 相等、卖方分量与成交额上界，不从累计重构逐笔优先级；LiveOrderReservations 保留剩余 qty、continuous/auction 共用汇总与 checked 溢出。正式依据继续由 trading-rules 的交易所/财政部原始链接及核对日期承担；中国结算费用表访问失败仍是原有证据限制，本复核没有重新联网确认，不能称本日官方规则重新核验通过。
- **必要性与范围：** 实现围绕所分配 owner 迁移，临时 context/audit/reservation 不新增权威状态、存档 schema 或依赖。OpeningFigures 保留 code+shares 默认精确匹配、双账套不同债务行、科目顺序与元到分。CumulativeFeeAuditV2 的 Result<bool> 保留外层委托身份错误上下文，属于已交接的最小签名调整。可选动作按指定权威动作集核对，不另扩行为修复。
- **边界、跨层与复杂度：** 新增静态可见测试覆盖风险等号/时钟/无 peak、散户 T+1/成本/零 stop/分钟样本/RNG、开局双投影/小股本失败、存档时钟/首错、费用零负值及 snapshot 混合来源/溢出。SaveValidationContext 删除的重复时钟计算在主流程已用同公式且更早 checked，未发现有效输入接受范围或首错变化；serde/JSON/TS 字段保持。上述两处 caller 漏迁移与投影测试缺口须完成修复与再次复核。

本记录仅证明指定 leaf diff 的独立静态审查；不证明编译通过、TDD red/green 闭环、完整 session roots/P9 门禁或长期统计验收。

## 修复后再次复核

同日 worker 局部修复后已再次只读核对：

- 发现 1：fixture 已恢复 `session.poison`，原 poisoned-save 拒绝断言未变，解决。
- 发现 2：`validate_plan_contract` 的 TradingPlan/linked 读取已全部改 getter，`stock_codes.contains(plan.code())` 与 `linked.code() != code` 保持借用及原比较；ParentOrderPlan 条件未改变，解决。
- 发现 3：已新增 `strategy::zi_noise::projection_tests::strategy_data_projects_all_individual_retail_parameters`，以非默认实例逐字段核对完整本人参数、浮点 bits 和 profile，静态验证缺口解决；实际执行仍交 parent。
- worker 同时完成父级所分配的 Account/Position caller 接线：快照读 immutable getter；v2 恢复以 `restore_strategy(Some(strategy))` 写回先暂存、后安装的 accounts；未改变 wire 字段、恢复顺序或原子边界。
- 再次执行限定 leaf 范围 `git diff --check` 成功。

指定 leaf 范围没有剩余静态阻断发现，三门静态复核通过。此结论仍不替代 parent 统一编译、短测试与剩余 session roots 的完整复核。

## 最终编译 caller 增量与冻结 SHA

按 parent 的 compile02/compile04 增量通知，再次独立核对 `persistence/v2_tests.rs`：两处 ParentOrderPlan 字面量改为 `from_saved_facts`。构造器只逐字段安装传入事实，没有新 guard；首处原参数依次保持 `code/Buy/100/0/100/None/None/None/1000分/480`，第二处保持 `stock/side/50100/50001/50100/Some(order)/Some(1)/None/1分/480`。active id 与 active remaining 仍为独立 Option；没有 zip 丢失或提前修补 fixture。SaveParentOrderPlan 的 wire DTO 字面量保持原样。

三个断言改用 `active_child_remaining_qty()/target_qty()/filled_qty()`，getter 直接读取对应字段，预期值仍为 `Some(1)`、`50100`、`50001`。原剩余委托数量和 envelope audit 对照断言保持。增量是私有字段 caller 迁移所必需，没有改变大 A 交易语义、序列化接受范围或测试强度。

14 个指定 leaf 源文件的当前内容 SHA、行数和完整 binary diff SHA 已记录于 [leaf-source-manifest.json](leaf-source-manifest.json)，以 parent 的 63 文件 [source-manifest.json](source-manifest.json) 为范围来源。该清单不覆盖 roots 或其他 group；内容漂移需要再审。

- `persistence/v2_tests.rs` SHA-256：`2f84d83557af16f2c00036cc904d1b556efe39c2f5261d8ed1dde23951393032`。
- 指定 14 文件完整 `git diff --binary <baseline> -- <paths>` SHA-256：`ad938b9b0bf753c8a59a3b831ec8e02df773254c7980f5218f309e869ade2965`。
- 限定 14 文件 `git diff --check` 再次成功；未运行 cargo 或产品测试。

最终增量静态复核通过，无新增有效发现；按所记录 SHA 冻结 leaf 审查证据。
