# 隐藏候选独立裁定 05

## 范围与依据

核对 `batch-154.md`、`batch-156.md`、`batch-161.md` 的结论及其指向的现行 producer/consumer。基线为 `43b1aa5`。对照总账 G01–G68、Q01–Q23、`exhaustive-review/resolution.md`、`reaudit-accounting-contracts.md`、`reaudit-core-contracts.md` 及候选裁定 01–04；接受的领域依据包括 ADR-0013、ADR-0019、ADR-0025、公司计划及当前 consolidation DTO 注释。未运行测试或构建，不改产品代码。

## 候选裁定

| 观察 | 现行证据 | 裁定与去重 |
|---|---|---|
| `InventoryLedger::receipt` 成本溢出时数量先增加；工业采购/生产 caller 先过账再更新子账 | `inventory.rs` 的 `InventoryItemState::receipt` 先写 `quantity`，之后才 checked-add `total_cost`；现有测试明确断言 Err 后数量已增加。`industrial/purchasing.rs` 与 `production.rs` 在 `receipt` 前完成账套过账。`reaudit-accounting-contracts.md` 已说明 `Books::post_batch` 的原子范围仅覆盖 Journal/余额，不涵盖后续子账应用；行业 operation 可保留此前阶段结果，session 日终另由外层候选回滚。 | **不升为新 G/Q。** 这是可观察的低层部分写入，但没有找到 `InventoryLedger::receipt` 或完整工业 operation 失败时全状态不变的已接受保证。错误以 `Result` 显式返回；checked 溢出本身不证明违反强原子承诺。与总账 Q17 的 `ClosingEngine::correct` 不同对象，但沿用同一“不把 Journal 批次原子扩大到所有后续子账”的边界；不并入 G35/G36，也不因此核销它们。若要要求公共行业 operation 全事务，需另立明确契约。 |
| `TradeOpenLedger::write_off` 清零开项后，核销累计额 checked-add 可能失败；工业 caller 先过账 | `receivables.rs::write_off` 先设 `open_amount = ZERO`，再增加 `written_off_total`；`industrial/sales.rs::write_off_receivable` 先 `post_with_commit`，后调用子账 `write_off`。既有会计复核已将类似“过账成功、后续子账 apply 失败”定为旧底层边界，并指出没有整项行业事务承诺。 | **不升为新 G/Q。** 代码顺序证实失败后状态可部分变化，但目前没有被违反的强原子契约。不得仅以 checked-add 返回错误或 caller 先过账升级缺口；也不与 Q17 合并。 |
| 合并往来允许配对的零/负 `IntercompanyBalance.amount` | `accounting/consolidation/worksheet.rs` 对 DTO 的定义明确称金额“恒正”，`WorksheetLine` 注释要求正金额；但 `consolidation/eliminate.rs::precheck_balance` 未检查 `amount > 0`，`balance_entry` 只检查双方相等及资产/负债形状，随后直接生成工作底稿行。公开请求可提供对称零额或负额，越过现有 producer 校验进入结果。 | **确认独立校验缺口，建议新增候选/G。** 这有直接类型文档契约，不是从真实会计制度推导；范围限定为 consolidation API 必须拒绝违反现有恒正 DTO 约定的输入。与 G28 的固定集团交付、成员账面上界及分类冲突均不同；G28/Q17 和 G/Q 其余项均不覆盖输入金额符号校验。无须据此主张任何实际集团已发布错误报表，也不改变沪深交易语义。 |
| `record_fill_dated` / `observe_position_dated` 遇 legacy writer checked 溢出时保留部分 legacy 字段变更 | `experience/feedback/lifecycle.rs` 在 legacy writer 成功后才推进 feedback；`experience/position_transition.rs` 的 retail writer 先更新交易/观察时间、峰值等，再执行失败计数 `checked_add`。溢出可使公共 dated writer 返回 Err，但此前 legacy 字段已有变化。G08 的当前定义是散户生产消费仍走 legacy writer，dated writer/衰减未接入消费链；G43 是淡出股票仍进入 root 候选。 | **不并入 G08/G43；暂不升新 G/Q。** 此处是显式 dated API 的失败原子边界，不是 G08 的日期/衰减生产接线，也不是 G43 的候选资格过滤。ADR-0013 定义散户事实和日期衰减语义，但未规定 dated writer 的任何 Err 都必须整对象回滚；存档可编辑原则也未建立该方法的事务承诺。保留为独立边界观察，不能因 G08/G43 相邻或未跑测试列为缺口。若后续确定方法契约要求失败无状态变化，应独立登记并验证 legacy 与 feedback 全字段不变。 |

## 既有条目与排除项

- `batch-156` 的费用文案、`CalendarPolicy.source_digest`、`CivilInstant`、`GameConfig`、经历状态校验及 `AppendOnlyHistory` 观察不在本次新增裁定范围；`CivilInstant` 与 digest 已由候选裁定 04 单独处理，本记录不重复建项。
- `batch-161` 的计划协调与存档发布边界没有新 G；协议候选身份不能扩展成跨运行订单优先级。G16、G39、Q17、Q22、Q23 均按总账原定义保留，不因本轮 OOP/低层失败边界核销。
- 三份来源中的对象归属/保留判断不构成新实现承诺；未执行测试不作为实现缺失证据。

## 汇总结论

本批唯一有现行显式契约支撑的新候选，是 consolidation 对正额 `IntercompanyBalance.amount` 的缺少拒绝校验。Inventory receipt、receivable write-off 与 dated retail experience 的部分写入路径是真实源码行为，但现有材料未证明相应低层 API 承诺强原子；它们不升 G/Q。散户经历部分写入既不等于 G08 的生产接线缺失，也不等于 G43 的淡出过滤。无 A 股交易规则变更；未作官方会计规则现行性复核。
