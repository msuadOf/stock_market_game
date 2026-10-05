# Session assembly 测试迁移独立复核

## 范围与结论

完整阅读 `packages/engine/tests/session_company_assembly.rs`、归档原测试 `agents/company-system/legacy-session-assembly-reference.rs`、迁移说明 `agents/company-system/session-assembly-test-migration.md`，并逐项对照 `packages/engine/src/session/company_groups/tests.rs`、`packages/engine/tests/consolidation/gold.rs` 与 `packages/engine/tests/consolidation/intercompany_gold.rs`。

迁移边界正确：当前测试是新的 `Simple` 真实 `GameSession` 测试，不能视作旧的 `CompanyOperations`／集团 Session 测试原样迁移。旧测试依赖的 `SessionSetup.groups`、`company_operations` 与 consolidated `closing_registry` 属于旧 `Simulation` 路径；把这些拼成 `Simple` 后台或让新测试承接旧集团断言都会改变测试对象。归档保留原 3 个测试及其断言，迁移说明明确未声称旧 Session 集成覆盖已迁完。

## 逐项断言对照

| 归档旧验收 | 当前 Simple 测试 | 独立真实账务覆盖／边界 |
|---|---|---|
| 四行业经营公司真实账簿月结、Standalone 月报、公开查询、恢复后推进 | 四个显式 `CompanyKind` 的发行人/股票/股本映射；真实 Simple 财务月结、月报版本、Standalone 公开报告；保存恢复后推进 | `company_groups/tests.rs` 有真实历史集团账簿及月报、恢复对照，但不等于四行业经营 Session 覆盖。Simple financial reports 是新 Simple 契约，不应冒称旧经营断言替代完成。 |
| 混合行业集团 80/20 持股；合并季报关闭、延迟发布；公开报告含 distinct report IDs 及 Consolidated scope；恢复后保持报告 | 当前 Simple 测试没有集团范围、集团关账或集团发布断言 | `consolidation/gold.rs::mixed_industry_group_80_20_minority_split_is_exact` 独立验证工业+银行的 80/20 少数股东损益/权益、scope、账目与现金；`company_groups/tests.rs::custom_monthly_group_publication_requires_real_member_closing_and_keeps_scope` 验证需要成员结账及发布范围。但没有替代旧 Session 延迟季报发布、distinct ID、恢复后公开查询链。 |
| 可编辑存档中的母子公司真实内部销售，经 Session 结账抵销、发布合并报告并恢复 | 当前 Simple 测试不构造经营对手方、内部销售或集团 | `company_groups/tests.rs::historical_group_balances_survive_real_trade_settlement_and_restore` 以真实母子账簿交易、结算、历史余额及恢复生成合并月报；`consolidation/gold.rs::fully_sold_intercompany_sale_has_no_inventory_line` 与 `intercompany_gold.rs` 上游/下游金样分别验证内部销售抵销、未实现利润方向和少数股东影响。它们是独立账务测试，不是旧端到端 Session 发布/恢复用例。 |

## 保留的未来门禁

- 旧归档第三例中的 Session 组装、报告发布与恢复贯通仍无当前 Simple 对应契约；`Simple` 当前不表达 consolidated scope。未来只有正式 `Simulation` Session 能力形成时，才以归档完整断言恢复对应验收。
- 旧例的精确旧 raw cents 结果、同日 distinct report ID 以及查询页大小／分页行为没有由上述独立账务金样证明；应继续作为相应 Session／查询契约的未来门禁，不能记为当前通过。
- 低层 consolidation 金样证明其各自的计算断言，不证明整条宿主验收、完整测试套件通过或全部历史 Session 断言已迁移。

## 必要性与范围

当前新测试对 `Simple` 的四类财务 kind、发行人映射、月结、Standalone 查询和恢复续行有直接价值，范围与其名字及新契约一致。未发现通过伪造集团数据、影子 `Simple` 后台 Ops 或删除旧断言来满足编译的问题；旧断言完整留档。审查没有运行测试；本记录不宣称测试通过。未改代码、Cargo 配置或 coverage index。
