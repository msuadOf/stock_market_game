# 批次 197：全文核验记录

## 基线、约束与来源

- 基线为 `43b1aa5`；源树 `/data1/baiyifan/workplace/stock_market_game`，caller `/data1/baiyifan/workplace/stock_market_game/.worktree/implementation-reaudit`。
- 按连续全文读取至 EOF；三份来源的实际行数与 SHA-256 均和唯一扫描计划相符。未运行测试、构建或 Git 命令，未改产品代码。
- 对照 caller `AGENTS.md`、`docs/principles.md`、`docs/decisions/0016-fundamental-factor-model.md`、现行实现缺口总账及 `reaudit-engine.md`；历史材料的产品建议不视为已实施契约。

## 来源复核

| 来源（行数；SHA-256） | 全文范围 | 复核意见 |
|---|---|---|
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-strategy-01-delta-final.md`（14；`c5e834be970f6135cfd09b01042657ba79d88ed07aa52f5f268b584d087dc723`） | 全文：D01/D02 调查 delta 范围与结论 | D02 作为未实施的期限缺陷线索迁出 OOP action；其 serde、PlanBook 恢复及期限计算边界与 caller 代码相符。文档明确不宣称修复或验证完成。 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-strategy-01-fields-final.md`（15；`235ff426983ab1d14ac84813645d51b53f0e4aa23b6d8afaf585f33425ab1d87`） | 全文：TradingPlan 字段清单 binding、适用范围与三项门禁 | 精确字段清单是文档候选，不是源码实施；receiver 与 PlanBook 委托保留计划生命周期语义。D02 期限溢出仍作为独立边界，未被字段收口解决。 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-strategy-01-final.md`（56；`9354070e21adbab2972bb78a90a94a045dc6ea8cacc03624e7bdd68907508b18`） | 全文：A03 阻断项、23 文件逐项核对、语义及验证范围 | 历史终审认为期限校验遗漏了公开构造、serde 与簿恢复路径，并要求 checked 算术边界。旧稿“TradingPlan 全字段为 pub”的事实不再适用；当前字段限定为 `pub(in crate::plans)`。期限缺口仍由 serde 与恢复/查询路径支持。 |

## Caller、owner 与 consumer

- `TradingPlan` 是期限字段和 `last_valid_trading_day` 的状态 owner。`packages/engine/src/plans/state.rs:164-180` 显示它派生 `serde::Deserialize`，字段为 `pub(in crate::plans)`；不沿用历史终审关于 crate 外直接改字段的理由。`state.rs:337-339` 仍以裸 `created_trading_day + u64::from(horizon_trading_days) - 1` 计算有效期末日。
- `PlanBook::from_parts` 在 `packages/engine/src/plans/mod.rs:244-283` 重建计划索引并验证若干存档不变量，但没有验证期限非零或有效期末日可表示性。独立 serde 构造与此后计划簿恢复因此仍需有效性契约覆盖。
- 直接 consumer 包括 `packages/engine/src/plans/revision.rs:71-76,302-306,323+` 的事件期限守卫/到期路径，以及 Session 生命周期扫描、紧迫度与决策链读取 `last_valid_trading_day` 的路径。`packages/engine/src/session/persistence.rs:1409+` 的 `validate_plan_contract` 目前检查账户、证券代码、目标数量/成交进度，未检查期限。
- 历史建议中的边界为 horizon=0 显式拒绝，先 `checked_sub(horizon, 1)` 再 `checked_add(created, offset)`；`(u64::MAX, 1)` 可得 `u64::MAX`，`(u64::MAX-1, 3)` 应显式失败。这属于建议的错误与恢复契约，不应写成现状或未经批准的 A 股规则。

## 总账、ADR 与候选界限

- 现行总账已在 `implementation-audit-2026-10-02.md` 的 G69 登记同一期限恢复/查询缺口，包含独立反序列化、`PlanBook::from_parts` 与 Session 计划校验遗漏、裸算术风险；本批不新增重复 G，也不自行更改 G69 状态。G16 是计划簿历史复制成本问题，与期限输入校验不同。
- ADR-0016 §50 将计划期限纳入 NPC 持续交易意图，未规定特定 u64 上界或交易制度规则。期限可表示性属于 engine 存档/API 防御边界，不改变 A 股订单有效期、撮合、T+1 或申报单位。本批没有新的交易所法源主张。
- 未决与反证：历史终审关于 crate 外任意字段改写的论据已过时；没有运行坏存档来复现崩溃，也没有证明实际存档已含非法 horizon。PublicLibrary D01 与本批领域期限问题分离，不从旧复核扩大本批候选。
- 结论：三份来源支持现行 G69 所记录的期限有效性候选；仅作独立原文核验，不升级或核销 G/Q，不声称实现完成。未运行测试或构建。
