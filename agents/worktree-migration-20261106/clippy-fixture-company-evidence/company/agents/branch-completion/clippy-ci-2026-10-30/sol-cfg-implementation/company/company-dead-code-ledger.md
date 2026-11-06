# company Clippy dead_code 修复台账（2026-10-30）

基线：`50d493701aa0fbb7d62563855a84dc07c89c29b0`。证据：`agents/performance-delivery/formal-runtime-sync-2026-10-30/clippy-50d493.stderr` 与 `clippy-50d493-diagnostics.json`。本轮未运行 Cargo/Clippy；静态核对全部源码引用，测试由父级统一排窗。

## 精确 test 配置

- `company/operations/core.rs` 的 `correct_company_report_for_session`、`LegacyDisclosureCorrectionCtx`、prepare 入口与 `PreparedLegacyDisclosureCorrection::into_source` 仅由 `operations/disclosure_correction_legacy_tests.rs` 调用；该测试模块为 `#[cfg(test)]`。生产更正走 `company/correction.rs` 的批量 `prepare_disclosure_corrections`，由 `session/company_corrections.rs` 消费。旧 mutating bridge 保留其测试行为，限定为测试构建。`PreparedLegacyDisclosureCorrection` 的 `source`/`proof` 只读访问仍为生产可见，因为 `session/disclosure_candidate.rs` 的 source enum 直接引用该类型；该 re-export 和承载类型不门控。
- `company/disclosure_correction_proof.rs::ConfirmedDisclosureCorrections::from_owner_result` 仅被 proof/owner 单元测试及上述 test-only bridge 调用；正式 batch 链仍使用 `from_owner_results`，未门控。
- 各行业税务 owner getter/install adapter 与 `operations/config.rs` 的行业转发仅供上述旧测试 bridge 使用，限定为测试构建。`bank::validate_owner_state`/`add_balance`、工业和地产税务状态验证仍由原单元测试调用；`real_estate/owner_state` 模块仅由其测试验证，故仅在测试构建编译。生产实际税务计提/支付和各行业本身的状态校验保留。
- `company/simple/preferences.rs` 的提案评估算法、其私有输入/结果类型、频率与幂等 ID 辅助、限跌敞口计算及两个比例常量，在当前完整 engine 源码里只有 `preferences_tests.rs` 调用。按父级核定的当前交付边界，评估逻辑与自动日结提案执行链属后续实现；本轮不新增生产日结链，也不把偏好配置、校验、持久化 ledger 或供显式提案使用的 `preference_ex_dates` 改为 test-only。自动评估的专属计算只在测试配置下保留完整源代码与既有强断言。
- `CompanySystem::history` 只由 Session/company-source runtime 单测读取，test-only；内部日历事实及 `advance_day` 仍是生产路径。

## 无消费者而移除的最小内部入口

- `operations/config.rs` 的 IndustryBooks 税务重述/计提/支付转发在整个 `packages/engine/src` 没有调用点；生产结算在 `operations/settlements.rs` 直接按实际行业处理器分派，删除无用转发，不改行业会计逻辑。
- `cash_dividend_tax.rs` 的 `redenomination_by_event`、`stock_bonus_bases`、`stock_bonus_basis_by_event`、`receipt_by_event`、`dividend_event_by_id` 五个 accessor 在生产和测试均无调用点；底层事实、校验、序列化字段及仍有调用者的 `tax_day_receipts`/`payment_by_id` 保留。删除这些声称供 Session 查询但当前未被 Session 使用的窄入口。
- 工业科目表改用本文件已有的 `acct::TAX_EXP` 常量生成相同 `6801` 科目定义，保留常量的领域用途与原 A 股外无关的账务语义不变。

## 保留并上报的诊断边界

`disclosure_correction_test_fixture.rs` 的 Clippy 诊断按任务边界明确未触碰；`query.rs` 与 `disclosure_roe.rs` enum 未触碰。`simple/preferences.rs` 内配置/ledger/ex-date 生产契约未弱化。任何父级裁定之外的死代码诊断不自动扩大到业务语义或测试断言。
