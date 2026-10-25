# AnnouncementContent 严格解析交接（2026-10-25）

- 工作树：`.worktree/announcement-content-parser`；分支 `fix/web-announcement-content-parser`；基线 `b172c1ab`。
- 仅新增 `apps/web/src/save/schema/company/announcement-content.ts` 与相邻测试。不接线外层 `Announcement`，该部分由 Web writer 负责。
- 公共接口：`parseAnnouncementContent(value: unknown, path: string): AnnouncementContent`。导出可辨识 union，未使用 `any` 或类型断言冒充解析。
- 六种负载以 `packages/engine/src/information/publication.rs` 当前 serde 为依据：`Shock`、`CashDividend`、`RightsOffering`、`IssuerRepurchase`、`ShareSplit`、`StockDistribution`。
- `CashDividend`/`RightsOffering`/`IssuerRepurchase` 的 `value` 包装 `plan`；`ShareSplit`/`StockDistribution` 的 `value` 为方案本体。分红另外保留 `total_gross`。
- 金额使用 `Money` 整数分字符串（复用 `primitives.money`），股份与百万分比例使用 canonical u64 字符串；`CompanyId` 是 u32 number，方案 `StockCode` 是 string，`HolderId.Account` 是 u64 string。参考 ADR-0031、ADR-0039、ADR-0041 与现行 Rust 字段 serde，不变更交易规则。
- 复用 `primitives`、`personal/common` 和 `policies/shock`。只为已有 `decimal` 增加局部 canonical 检查，未新建解析框架。
- 本层校验 wire 形状与 scalar 域；方案业务关系、交易日历与受理能力由 engine 恢复边界校验。`StrategyBased` 保留当前已声明 enum，未宣称支持其执行。`PaymentFailure` 与外部 `occurred_on` 的关系由外层 writer 维护。

## TDD 和验证

1. 先写测试与抛出“AnnouncementContent 尚未实现”的占位函数，再执行带 10000ms 外 supervisor / 10000ms case / concurrency=4 的聚焦命令；17 case 全红，原因均为尚未实现（初次工具输出 chunk `e3e58a`）。当时未另落盘原日志，不能把本记录称为原始日志。
2. 实现后 17 case 全绿；与 `urgency-policy.test.ts`、`npc-attention.test.ts` 共 22 case 全绿，实际由 3 个 Node 文件进程并发执行。
3. 增加递归字段和枚举边界后，共 19 个新增 case、邻近 5 个 case 全绿；最新可复跑原日志在 `short-tests.log`。命令：

```sh
node scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-concurrency=4 apps/web/src/save/schema/company/announcement-content.test.ts apps/web/src/save/schema/urgency-policy.test.ts apps/web/src/save/schema/npc-attention.test.ts
```

4. 独立 `tsc --noEmit --strict --esModuleInterop --allowImportingTsExtensions --target ES2022 --module ESNext --moduleResolution Bundler --skipLibCheck --typeRoots /data1/baiyifan/workplace/stock_market_game/apps/web/node_modules/@types --types node` 对两文件通过，外层 `run-long-validation.mjs 300000`。初次缺 `node` types 路径失败，随后测试 fixture 的可选 `plan` narrowing 失败，修复后通过；未宣称全 Web 类型检查通过。
5. 对两新增文件 `oxlint --deny-warnings` 与 `git diff --check` 通过。类型与 lint 检查通过独立并发进程执行；这些短检查未形成长跑。
6. 未运行全量验收、浏览器或 Rust 测试；未修改真实 producer JSON；未进行 merge 或 push。独立完整 diff 复核由 root 指派。
