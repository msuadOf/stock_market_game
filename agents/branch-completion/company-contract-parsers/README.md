# F4a 公司查询严格 parser 冻结证据

本工作区：`.worktree/company-contract-parsers`，分支 `feat/web-company-contract-parsers`，base `3984e14167202dc645ec340fbd2a73e661c71c8a`。本轮接管已中止 writer 的六个未提交文件；未覆盖整树、未改 App / 现有 host / protocol / store / generated / root / save 或经济实现。新增工作文件仅在本目录。独立完整 diff 复核交协调者安排；本文不是独审通过声明。

## 实现与契约

- `company-contract-views.ts`：`parseCompanyCapabilities` / `parseOwnerRightsOfferings` / `parsePeriodChangeExplanation`，导出对应三个类型。前两种已有 generated 类型直接引用；缺少 generated 的配股 view 从唯一严格 parser 返回类型推导，没有平行宽松 DTO。
- `dividend-tax.ts`：`parseOwnerFlatWithholdingReceipts(value, owner, path?)` / `parseAccountDividendTaxStatus` / `parseDividendTaxOutstandingViews(value, owner, path?)`；导出 `FlatWithholdingReceipt` / `AccountDividendTaxStatusView` / `DividendTaxOutstandingView`。
- `corporate-action-views.ts`：`parseQueuedRightsSubscriptions(value, owner, path?)` / `parseRejectedRightsSubscriptions(value, owner, path?)` / `parseSimplePreferenceRejections`；导出 `QueuedRightsSubscription` / `RejectedRightsSubscription` / `SimplePreferenceRejection`。另保留单条 `parseQueuedRightsSubscription`。
- owner 参数必须是规范 u64 字符串，支持 `0` 与非零 owner；含 account 的回执数组逐条复核本人，混入其他 owner 整次失败。能力摘要和完整配股 DTO 没有 account 字段，隔离仍由真实 Rust host 守卫负责。
- `CompanyId` 为 u32 number；公司域 `stock` 为当前 Rust 的非空 `StockCode` 字符串，绝不当作数值 market `StockId`。现接口没有 catalog 上下文，不能据字符串本身证明证券已配置；未知证券/公司存在性仍需宿主及后续目录接线保证，不映射到第一项。
- `Money` 为 i64 分规范字符串；会计金额为两位小数元字符串，边界按 i128 分检查；股数为规范 u64 字符串。`rate_bp` 为 0..10000 number，`operation_seq` 依 Rust `js_safe_u64` 为 JS 安全非负整数 number。
- 五类 readiness 各一次，拒绝重复、缺项、未知 enum、缺键/多键、错误类型、越界、空原因及无效 CivilDate（1900–2199）。不可用金额保留原因；不补零、不推算财务、不给未知身份兜底。
- outstanding 精确保留 `{ account, stock, outstanding: {numerator, denominator}, latest_collection }`。分子为非负 i128、分母为正 u64，均为规范字符串且分数必须已约简。无 collection 就是尚无划收事实；历史 collection 不与当前 outstanding 强行相等，不从当前余额推断资金不足。
- 回执只校验 Rust 已承诺的事实勾稽：FlatWithholding 按 bp half-up 到分；collection 的 available_cash−collected=remaining_cash；配股已结算记录 paid_shares+waived_shares=requested_shares。没有实现或开放新交易命令。

领域依据为本 base 现行 Rust DTO、ADR-0035 / ADR-0040 / ADR-0041 与既有公司规则。此 slice 只恢复数据边界，没有重新定义 A 股税务/登记/交易算法；FlatWithholding 继续是 ADR-0040 的游戏简化，不能把本 parser 当作真实税制或日终结算验收。源码与 ADR SHA256 见 `dto-source-sha256.json`。

## 红绿与验证

1. 接管时已有 `red.stdout` / `red.stderr` 原样保留。原日志 9 例 1 过 8 败，但对应实现是 `value => value` 透传 stub；该记录仅作历史诊断，不计业务 TDD 红灯。
2. 从 `18725d07` 的真实 parser 最小适配当前 DTO 和导出，未先修范围校验。`behavior-red.stdout` / `.stderr` 为实际可运行的 9 例 1 过 8 败：重复 readiness、u32/u64/i128/Money 边界等业务断言失败，无缺模块/编译错误。运行后、修复前保存 `behavior-red-source/` 六原件与 `behavior-red-receipt.json` hashes。`first-green` 为修复后 9/9。
3. 补充嵌套边界、跨 owner、税回执勾稽测试后，`receipt-red` 为 13 例 12 过 1 败（代扣金额不符却被受理）；`receipt-red-source/` 及 receipt 保存当时六原件。随后补既有回执勾稽，`green` 为 13/13。
4. 最终可复跑：`/data1/baiyifan/workplace/stock_market_game/.tmp/node24-env/node agents/branch-completion/company-contract-parsers/verify.mjs`。脚本并发运行 Node 测试、六文件 strict TypeScript、六文件 lint，独立任务 3 个；测试文件并发 3，case timeout=10000ms、每个命令进程树 deadline=10000ms。机器报告 availableParallelism=128；任务均不足 1 秒，没有长任务或持续 CPU 采样，不声称实际满核。
5. `final-receipt.json` 记录实际命令、时间、源 hashes、exitCode；`final-tests` 13/13，`final-typecheck` / `final-lint` exit 0。未运行 Cargo、build/all、全量回归、真实三宿主或浏览器旅程。

验证脚本初版误按 supervisor 返回 exitCode 读取并漏显式 Node types；对应 `typecheck-tests-first.stdout/.stderr` 保留实际 TS2591 环境诊断。随后脚本接回调却漏 `captureOutput`，`verify-callback-diagnostic.json` 保留失败；最终脚本明确 captureOutput、逐任务记录失败、显式 `--types node` 后绿。这些都不是业务红灯。

## 限制与后续门禁

测试输入是依据当前 Rust/ts-rs 源码构造的非空及边界输入，不冒称为真实 Rust serde producer 输出。commands 协调者已明确没有现成可宣称 producer 证据的 JSON，仍在只读检索；本片没有启动 Cargo 补产。因此真实非空 producer→Web 深等、三宿主 owner 守卫与同代目录存在性、query 接线、只读组件旅程仍属于后续组合门禁。Proposal / Correction 未冻结，不在本片恢复。F4b 已收到精确导出和单位口径；本地提交后由协调者派未实施 Sol 审完整 diff，发现修复后须再审。
