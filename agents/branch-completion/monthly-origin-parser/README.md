# MonthlyDisclosure parser 最小增量证据

日期：2026-10-26（Asia/Shanghai）。独占 worktree：`.worktree/monthly-origin-parser`；分支：`fix/web-monthly-origin`。
基线完整 OID：`f364e64cb481d011a620e99760386b5c743c1ec8`。原 I-B 冻结不变；本片只作为其后增量交独立复核，再给 I-D 组合。

## 范围与依据

- `apps/web/src/save/schema/company/reports.ts`：只补 `PublicationOrigin.MonthlyDisclosure` 类型及 parser 分支。
- `apps/web/src/save/schema/monthly-report-schedule.ts`：从现有 setup 私有 `schedule`/`delay` 机械提取共用严格 parser；`bounded` 保持同样校验与错误文案。
- `apps/web/src/save/schema/setup-company-options.ts`：只删被提取函数与对应 type import，改 import/call；其余语义、错误路径、i128 边界不变。
- `apps/web/src/save/schema/company/monthly-origin.test.ts`：独立的新 parser 测试。
- 本目录：本轮原始输出、命令、源码 SHA、失败与最终通过证据。未修改 generated、root/market/general stored schema、PlanId/OrderId、公司披露业务或模型来源。

Rust 依据是当前基线的 `packages/engine/src/information/publication.rs` 中 `PublicationOrigin` 的 serde 形状，以及 `packages/engine/src/information/monthly_schedule.rs` 的 `validate`/`instant`。SHA 见每轮 receipt。载荷直接是 `MonthlyReportSchedule`，不包 `ReportFrequency.Monthly`。

`Custom.day` 为 1..28，`second_of_day` 为 0..86399；Preset 仅 FirstDayEvening/TenthDayEvening；delay 仅 None/Uniform，Uniform.max_days 为 1..31；origin 的 delay_days 按 serde u8 为 0..255，seeded 严格 bool。实际 delay_days 超过 schedule 配置、公布时间不符、跨报表语义仍由 Rust 深验证负责；本片不把这些深验证冒充 shape parser 职责。没有补默认、兼容旧脏档或放宽原三分支。

本片是现有 engine DTO 的 Web 校验补齐，不改变沪深 A 股交易制度或法定披露规则；Monthly 是额外游戏报告，不能被理解为法定月报要求。本片没有新设交易规则，因此未新增官方规则研究或正式领域文档。

## TDD 与真实运行

正式短测命令由 `run-short-tests.py` 原样记录并执行：Node v25.8.2，外层 `scripts/run-with-deadline.mjs 10000` 进程树 deadline，Node `--test-timeout=10000 --test-concurrency=3`，三个文件各用独立 Node 测试进程。文件是新 monthly-origin、既有 setup-company-options、既有 company-schema；没有运行 Cargo、build/all 或 types:generate。

1. `initial-syntax-failure/`：首次新测试存在一个多余右括号导致解析失败，不能算业务红证；保留原始 receipt/stdout/stderr，没有改写输出。
2. `red.*`：修正括号后，19 case 中 14 过、5 失败；新增月报 case 的实际失败点为 origin 不受理或未进入该分支导致错误路径不符。实现尚未修改。整命令 1005.358ms。
3. `green-before-type-annotation/`：最小实现后 19/19 通过，1058.961ms。
4. `green.*`：测试合法 delay 矩阵补 `as const` 类型标注（断言与运行输入不变）后，最终 19/19 通过，951.704ms。红证与最终测试 SHA 因此不同，首轮 green 保留变化前的精确测试 SHA。`run-short-tests.py` 此时也删去冗余 Python 12s timeout，唯一测试命令期限仍是进程外 Node 10000ms。

原有两个测试文件及所有断言均未修改。`git diff --check` 通过。未运行完整 Web 测试或 TypeScript 构建，本片的短测通过不能外推为完整编译或整局旅程通过。

## fixture 与待验范围

新测试明确手构 PublicLibrary/PublishedReport DTO，使用独立最小字段，不从旧报告改 origin 冒称真实 Rust 月报生产存档。基线 `apps/web/src/save/fixtures/*.json` 未检索到 MonthlyDisclosure。受 Cargo slot 限制，本轮没有生成真实 Rust 月报 origin/report/save；此 producer 证据缺口原样保留，等待后续正式窗口。

Simple/月报完整旅程仍依 selected-source/R4 真实生产交付。新增 parser 用例也刻意接受 delay_days=255 的 u8 shape，不能据此宣称其能通过 Rust `schedule.instant` 深验证。

本片冻结后须由未实施者审完整 diff，核对大 A 语义、必要性、边界与跨层一致性。实现者不自行宣称最终验收完成。
