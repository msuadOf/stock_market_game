# SessionSetup 公司选项 parser 实施记录

- 工作树：`/data1/baiyifan/workplace/stock_market_game-setup-options`；基线 `f046075972bb3da42aa1fb4891bee83efd863cb7`。
- 范围：新增 `setup-company-options.ts` 及测试；只把现有 `parseJournalLine`、`parseTaxPolicy` 改为 export，复用其结构解析。`market.ts` 与全档接线由 planweb 后续 cherry-pick。本记录不宣称全档恢复完成。
- 8 字段（初派 9 为计数口误，已与 planweb 核对）：`company_system`、`dividend_tax_mode`、`flat_withholding_bp`、`rights_offering_enabled`、`issuer_repurchase_enabled`、`par_value_per_share`、`auto_corporate_foundation`、`report_frequency`。
- `company_operations` 与 `groups` 已有 parser 接线，不在新增字段范围。

## 契约依据

- `packages/engine/src/session.rs:1207–1247`：上述 8 字段的实际 Rust 类型；3 个开关是 `bool`，`skip_serializing_if=Not::not`，仅省略或 boolean，拒绝 generated 中误宽的 null。其余 `Option` 字段 `skip_serializing_if=Option::is_none`，canonical SaveSlot 以缺席表达 None，不填默认或 null。
- `packages/engine/src/session.rs:1527–1667`：非公司局残缺公司字段拒绝；公司局税制三态配对与正面值要求。`report_frequency` 独立于公司系统。
- `packages/engine/src/company/config.rs` 与 `simple/{config,period,finance_config,preferences}.rs`：Simple/Simulation、4 SettlementCycle、Fixed/Persistent、RevenueRatio/Growth、finance/preferences 结构。`CompanyId` 是数值 u32；Money 保留 i64 分字符串。两项送转比例的 u64 使用实际域 1..=10000000 / 1..=1000000000，因此 generated 的 number 不丢精度。
- `packages/engine/src/information/monthly_schedule.rs`：Preset/Custom、两个 preset、None/Uniform，日期1..28、当天秒0..86399、延迟1..31。
- `packages/engine/src/accounting/tax.rs`：显式配置 TaxPolicy 域；不把 Fixture 税率宣称为官方税制默认。
- 此批只验证既有 wire 契约，没有新增或修改交易制度。A 股语义沿现有 ADR-0035/0036/0038/0039/0040 与 `docs/trading-rules.md`；Simulation 在 wire 层可解析，是否可创建仍由 Rust 明确拒绝，不偷偷改为 Simple。

## TDD 与验证

- `setup-company-red.log`：6 个行为测试在空投影 stub 上全部因断言失败而红；未以 import/编译错冒充红灯。
- `setup-company-boundaries-red.log`：新增宽度边界用例在 TaxPolicy.version=2^32 上缺少异常而红。
- `setup-company-green.log`：8 cases 全绿。每 case 10000ms；整命令由 `scripts/run-with-deadline.mjs 10000` 进程外监督，Node `--test-concurrency=4`。各 case 不共享可变 fixture，路径断言指向实际输入字段。
- `setup-company-validation.log`：tsc 与 oxlint 独立 child 同时启动；共享300000ms上限，lint单独10000ms、`--threads=4`；tsc的依赖图为工具单进程限制。两项通过，wall 7760ms。
- 首次把 `run-with-deadline.mjs` 普通入口用于300000ms类型检查，被普通测试上限门禁拒绝，记录于 `setup-company-types.log`；随后改为正式 `runBoundedCommand` 进程外 child supervisor，未放宽普通测试上限。
- `setup-company-producer.log`：直接读取未修改 `/data1/baiyifan/workplace/stock_market_game/.tmp/plan-identity-target/producer-save.json`，对8字段实际存在项与parser输出深度相等；记录 source SHA256。未改写生产档、未验证全档其余字段。
- 未运行 all、merge、push；最终组合全档/类型/构建门禁归 planweb 与 root。

## 审查状态

已提交候选供未参与实施的 reviewer 独立检查；本记录不能替代审查结论。最终 cherry-pick OID 与组合验证须由 planweb 绑定，不把本树候选测试冒充组合树门禁。
