# CompanySystem 杂项测试契约适配

本记录对应新 `CompanySystem` 配置和成交额 `u128` 类型变更后的测试编译适配，范围仅限：

- `packages/engine/tests/support/event_mapping.rs`
- `packages/engine/tests/session.rs`
- `packages/engine/tests/auction.rs`
- `packages/engine/tests/company_decision_session/main.rs`

事件身份映射为 `PublicTrade` 和 `PrivateEventOmitted` 增加显式 variant、phase、entity 与 source 映射；未添加通配分支，也未过滤事件或改动 sealed gold hash。两处基于小 fixture 的成交额账目对账将 `u128` 通过 `i128::try_from(...).expect(...)` 显式转换。原有 `u64::MAX` 恢复边界值保留原数值并转换为 `u128`，不扩展或弱化该边界。

修复 `auction.rs` 中 `retain_stock` 对 fixture 的字段作用域，并导入 `WithinKindDistribution`。未修改生产代码、Cargo 配置、索引文件或测试断言。验证由主代理统一进行；本记录不声称编译或测试通过。提交前需由未实施者独立审查完整 diff，复核大 A 语义、必要性和边界覆盖。
