# 股数与现金分红 wire 独立复核

复核日期：2026-10-06。复核者未参与实现。范围为 `share_registry.rs`、其测试、`cash_dividend.rs` 的持股数 wire 字段及测试；按交办不改产品实现、不运行 Cargo。依据已读仓库 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、`docs/trading-rules.md`、ADR-0035，并查看相关 diff 与完整目标源文件。

## 结论

现有实现对 share quantity 的 wire 编码符合精确性要求：`ShareLot.qty`、`RegistrationSnapshot` 与 `ShareRegistry.issued_shares`、以及 dividend `Entitlement.shares` 都使用既有 `canonical_u64_decimal`；其强制 JSON string、规范十进制格式并通过 `parse::<u64>` 拒绝越界。`DayNetChange.change` 的本地 codec 仅接受有符号规范十进制 string，拒绝 JSON number、前导零、`+`、`-0` 和越界值，`i128::MIN` 的语法路径不会做取绝对值或取反，因此不会因边界值 panic。

`ShareRegistry` 的 `RegistryState`、`SnapshotState` 都复用上述 codec；嵌套 `ShareHolding/ShareLot`、注册快照 holdings、日结 receipt request、receipt disposals 中 lot qty 均有字段级覆盖。大数测试以 `u64::MAX` issued capital 和大于 `2^53` 的转让股数验证精确字符串及恢复，且检查卖方 lot 清零、买方数量和 disposal lot qty；现金分红 entitlement 也验证大于 `2^53` 序列化与拒绝 numeric、前导零、正号及 u64 overflow。审查日志 `.tmp/company-system/session-actions/fresh-2.log` 与 `fresh-3.log` 分别记录 registry 14 项、cash dividend 16 项通过；这是既有日志记录，不是本审查运行。

本批仅将股数字段改为精确 wire 表示，不改 A 股股数单位、发行股本守恒、投资者登记持股或除息分红语义。正式 A 股交易制度依据无需由纯 wire 字段变更重新推导。

## 发现与建议

- **低风险测试缺口**：`canonical_i128_decimal` 的实现逻辑正确，但新增测试未直接覆盖负零、`i128::MIN`、`i128::MAX` 和 i128 越界字符串。建议后续补小型 codec 边界用例，尤其确认负零拒绝和两端 round trip；这不构成当前代码 correctness 阻断。
- `cash_dividend/tests.rs` 新 precision test 排版较长，但不影响断言强度或行为。

复核结论：未发现当前 diff 中的 wire 精度丢失、字段漏标、股数守恒问题或跨层 A 股语义漂移；可通过本次独立审查门禁，记录上述边界测试建议。
