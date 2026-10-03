# 隐藏扫描批次 14

## 读取记录

按 scan-plan 中 batch 14、owner=4 的三项冻结 source 顺序连续读取至 EOF。实测行数与 SHA-256 均匹配；每项 aliases=1。`file-index.md` 已由本批前序读取按连续区间覆盖至第 1211 行 EOF；`review.md` 与 `worker-instructions.md` 已复核全文。本批另外校验 canonical `domain/report.md` 的 hash，但它不属于本批指定 source。

| Source | 行数 | SHA-256 | 完整性 |
|---|---:|---|---|
| `agents/oop-refactor-audit/completeness-2026-10-03/domain/review.md` | 39 | `1c0ccba993caf5a24abdd1126f0447418ee78b58d251cb3010ae4b7f15cbd57a` | EOF，完整 |
| `agents/oop-refactor-audit/completeness-2026-10-03/domain/worker-instructions.md` | 1 | `0dd2206879d6d0ea580d19288db4a4bdc980e567c35237f178e311d2ab50adde` | EOF，完整 |
| `agents/oop-refactor-audit/completeness-2026-10-03/file-index.md` | 1211 | `5594ccf2f0b5a58776d3a7416cc8ba8e42cc856eb48555b00e221c016be6e989` | EOF，完整 |

复核 `review.md` 的结论、N01–N07 候选复核记录、三门结论、G/Q 边界及其来源说明。canonical `domain/report.md` 实测 265 行、SHA-256 `f0b4c38abc5ab7e98be2e11053c6f8100b5a1876a4705b2b5fe9f5a9e49d13fc`；它作为额外上下文，未替代三项计划 source。

## 调用矩阵

在产品基线 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad` 对照历史建议所指对象、生产 caller 与测试位置：

| 候选 | 当前实现与 caller | 结论 |
|---|---|---|
| N01 | `packages/engine/src/indicators.rs` 的 `KdjAccumulator` 聚合指标迭代状态，并由指标计算入口消费 | 已实现；不产生新缺口 |
| N02 | `packages/engine/src/orderbook.rs` 的 `Order::validate_progress` 在撮合入口校验订单进度 | 已实现；不产生新缺口 |
| N03 | `packages/engine/src/market.rs` 的 `MarketDelta` 及 `Market::apply_changed_orders` 限定盘口变更写入口；session pipeline 构造并应用 delta | 已实现。不能把“受限写入”扩大解释成公开任意 setter；历史报告保留盘口差异断言，未发现新的写口遗漏证据 |
| N04 | 银行、保险、地产配置在各自 `Books::new`/`InsuranceBooks::new`/`RealEstateBooks::new` 边界执行校验 | 已实现三行业守卫；只保留既定行业差异，不形成共用默认规则候选 |
| N05 | `packages/engine/src/accounting/reports/window.rs` 的 `Accumulator::add_current_worksheet` 承担累积及溢出校验；`consolidated_window.rs` 调用 | 已实现；不产生新缺口 |
| N06 | `packages/engine/src/accounting/tax.rs` 的 `TaxPolicy`/子 policy 行为由工业 Books 持有和消费，公开 facade 仍在 `accounting/mod.rs` | 已实现；历史修订已闭合 public facade、文件清单与测试路径，不支持再报一次遗漏 |
| N07 | `experience/feedback/lifecycle.rs` 的 `RetailExperienceState::observe_institution_position_dated` 与 `clear_stale_institutional_holding`；session caller 在 `session/institutional_behavior.rs` 传递机构专用阈值；边界测试在 `experience/feedback/lifecycle/institutional_transition_tests.rs` | 已实现；阈值职责与零售衰减策略分开，未发现 writer 遗漏 |

候选迁移顺序与测试/边界沿用其已修订报告：本批没有建议新的迁移或测试动作。上述仅为静态源码定位，不声称测试运行或运行时行为验收。

## G/Q 交叉与结论

- N01–N07 均属于已有重构建议的实现状态复核，不是新的行为缺陷；没有充分证据新增 G 条目。
- 不核销 G08：N07 使用独立机构阈值，不改变零售体验的衰减或失败策略。
- 公司会计简化及税务法源状态不因 policy 封装而改变；G35/G36/G58/G59、Q19/Q23 仍保持原边界。
- 未触及大 A 撮合、价格、单位、交易时段或税务规则；不改变交易语义，也不将候选存在视作规则验收。
- 未运行测试、构建或 Git 操作；未修改产品源码。
